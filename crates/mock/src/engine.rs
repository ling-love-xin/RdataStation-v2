use std::collections::HashSet;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use fake::rand::rngs::StdRng;
use fake::rand::SeedableRng;
use fake::Fake;

use super::generators::generate_cell;
use engine::duckdb::row_to_arrow::duckdb_rows_to_arrow;
use engine::duckdb::{DuckDBManager, TempTableSource};
use shared::models::QueryResult;
use engine::sql::{ColumnDefInfo, QualifiedTable, SqlEngine};
use crate::error::{MockError, MockResult};
use crate::models::{
    ColumnDef, ColumnDependency, ColumnMappingResponse, GeneratorConfig, Locale, MockConfig,
    MockExportFormat, MockGenerateResult, MockScenarioResult, MockScenarioTableResult,
    ReferenceDomain, ScenarioTemplate,
};
use crate::schema_map::{ColumnMapper, parse_data_type};
use crate::templates;

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
    /// 追加到既有表：调用方负责先校验「目标表存在且含临时表全部列」（报错更早、更好懂）
    Append,
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

/// 全局取消标志，用于中断正在进行的生成任务
static CANCEL_FLAG: AtomicBool = AtomicBool::new(false);

impl MockEngine {
    /// 请求取消当前正在执行的生成任务
    pub fn cancel() {
        CANCEL_FLAG.store(true, Ordering::SeqCst);
    }

    /// 重置取消标志（在每次生成前调用）
    fn reset_cancel() {
        CANCEL_FLAG.store(false, Ordering::SeqCst);
    }

    /// 检查是否已请求取消
    fn is_cancelled() -> bool {
        CANCEL_FLAG.load(Ordering::SeqCst)
    }

    // ==================== 数据生成 ====================

    /// 生成 Mock 数据（无进度回调）
    ///
    /// 根据 `MockConfig` 配置在 DuckDB 内存临时表中生成指定行数/列数的模拟数据。
    /// 内部调用 `generate_with_progress`，回调为空。
    pub async fn generate(config: MockConfig) -> MockResult<MockGenerateResult> {
        Self::generate_with_progress(config, |_, _| {}).await
    }

    /// 生成 Mock 数据（带进度回调）
    ///
    /// 分批次生成数据（每批 BATCH_SIZE 行），每完成一批调用 `on_progress(batch_idx, total_batches)`。
    /// 结果存储在 DuckDB 内存临时表 `temp_mock_{table_name}` 中。
    /// 返回 `MockGenerateResult` 包含预览 `QueryResult` 和耗时。
    pub async fn generate_with_progress<F>(
        config: MockConfig,
        on_progress: F,
    ) -> MockResult<MockGenerateResult>
    where
        F: Fn(usize, usize) + Send + 'static,
    {
        Self::generate_table(config, &[], on_progress).await
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

        Self::reset_cancel();
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
            if Self::is_cancelled() {
                return Err(MockError::Generation("生成已取消".to_string()));
            }
            let start_row = batch_idx * BATCH_SIZE as u32;
            let count = std::cmp::min(BATCH_SIZE as u32, config.row_count - start_row);

            let mut all_values: Vec<Vec<String>> = Vec::with_capacity(count as usize);

            for row_idx in 0..count {
                let global_row = start_row + row_idx;
                let mut row_vals = Vec::with_capacity(config.columns.len());

                for (col_idx, col) in config.columns.iter().enumerate() {
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

                    let nullable_ratio = config.columns[col_idx].nullable_ratio;
                    if nullable_ratio > 0.0 {
                        let rand_val: f64 = (0.0..1.0).fake_with_rng(&mut rng);
                        if rand_val < nullable_ratio {
                            row_vals.push("NULL".to_string());
                            continue;
                        }
                    }
                    row_vals.push(value);
                }

                all_values.push(row_vals);
            }

            let insert_sql = SqlEngine::build_insert(&table_name, &safe_col_names, &all_values);
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
            if let Err(e) =
                conn.execute_batch(&SqlEngine::build_insert_select(&target, temp_table, &columns))
            {
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
        let dt = parse_data_type(data_type);
        Ok(ColumnMapper::infer(column_name, &dt))
    }

    /// 批量列智能映射
    ///
    /// 对多列表名+数据类型对进行批量推断，返回 `Vec<ColumnMappingResponse>`。
    pub fn map_columns_batch(
        columns: Vec<(String, String)>,
    ) -> MockResult<Vec<ColumnMappingResponse>> {
        columns
            .into_iter()
            .map(|(name, dt)| Self::map_column(&name, &dt))
            .collect()
    }

    // ==================== 场景模板 ====================

    /// 列出所有内置场景模板
    ///
    /// 返回 6 个预定义模板：电商/HR/博客/金融/社交媒体/企业通讯录。
    /// 每个模板包含表结构、列定义、推荐 GeneratorConfig。
    pub fn list_templates() -> MockResult<Vec<ScenarioTemplate>> {
        Ok(templates::get_builtin_templates())
    }

    /// 按 ID 查找场景模板详情
    ///
    /// 返回完整的 `ScenarioTemplate`，包含该场景下所有表的列定义和推荐配置。
    pub fn apply_template(template_id: &str) -> MockResult<ScenarioTemplate> {
        templates::get_template_by_id(template_id)
            .ok_or_else(|| MockError::TemplateNotFound(template_id.to_string()))
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

    fn read_preview(
        conn: &duckdb::Connection,
        table_name: &str,
        limit: usize,
    ) -> MockResult<QueryResult> {
        let sql = SqlEngine::build_select_all(table_name, Some(limit as i64));
        let mut stmt = conn.prepare(&sql)?;

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

// ==================== 辅助函数 ====================

/// 标识符规范化（列名 / 表名共用）：非字母数字与下划线 → `_`，两侧去 `_`。
///
/// 生成期临时表的列名由本函数得出，**建表 / 追加写回时也必须复用同一算法**：
/// 临时表列名与目标表列名不一致时，`INSERT` 的列清单会直接报「列不存在」。
/// 列名规范化后为空（如全为符号）由调用方报错；表名则退到 `auto_table_*`（见 `sanitize_table_name`）。
pub fn sanitize_identifier(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect::<String>()
        .trim_matches('_')
        .to_string()
}

/// 生成器参数在**采样期会不会直接 panic**（空区间 / 权重和非正 / 除零）。
///
/// 为什么必须在**生成前**拦住：这些参数原样进 `rand` 的采样——
/// - `RandomInt` 反向区间 → `Uniform::new_inclusive` 失败（`expect` panic）；
/// - `RandomFloat` / `RandomDecimal` / `Words` / `Sentence` / `Sentences` / `Password` 的 `min..max`
///   是**半开区间**，`min >= max` 即空区间（`random_range` panic）；
/// - `Date` / `DateTime` / `DateTimeBetween` 走 fake 的 `(0..分钟差)`，区间不足一分钟也是空区间；
/// - `ForeignKey` 向空集合取随机索引、`Sequence` 做 `row_index % len`（除零）、
///   `Weighted` 抽 `0.0..total`（总权重非正）。
///
/// 后果不是「这一次任务失败」：panic 发生在**持有内存库连接锁**期间，锁会被毒化，
/// 该进程之后每一次生成都报 `poisoned lock`，用户只能重启应用（实测）。
/// 所以这里是「参数能不能采样」的**唯一闸门**，错误信息要能直接指导用户改哪个字段。
fn generator_param_problem(generator: &GeneratorConfig) -> Option<String> {
    match generator {
        // ===== 集合类：空集合 / 全零权重（生成期除零或空区间）=====
        GeneratorConfig::ForeignKey { values } if values.is_empty() => {
            Some("的「外键取值」集合为空：请在列编辑对话框里填取值（一行一个）".to_string())
        }
        GeneratorConfig::Sequence { values, .. } if values.is_empty() => {
            Some("的「序列取值」集合为空：请在列编辑对话框里填取值（一行一个）".to_string())
        }
        GeneratorConfig::Weighted { choices } => {
            if choices.is_empty() {
                return Some(
                    "的「加权选项」为空：请在列编辑对话框里填「值, 权重」（至少一个权重大于 0）"
                        .to_string(),
                );
            }
            if choices
                .iter()
                .any(|(_, weight)| !weight.is_finite() || *weight < 0.0)
            {
                return Some(
                    "的「加权选项」权重必须是不小于 0 的有限数（负数 / NaN 会让总权重变成非正，抽不到值）"
                        .to_string(),
                );
            }
            if !choices.iter().any(|(_, weight)| *weight > 0.0) {
                return Some("的「加权选项」权重全为 0：请至少让一个权重大于 0".to_string());
            }
            None
        }

        // ===== 随机区间类：空区间 =====
        GeneratorConfig::RandomInt { min, max } if min > max => Some(format!(
            "的随机整数区间是空的（min {min} > max {max}）：把 min 改小或 max 改大"
        )),
        GeneratorConfig::RandomFloat { min, max, .. }
            if !min.is_finite() || !max.is_finite() || min >= max =>
        {
            Some(format!(
                "的随机小数区间不是有效区间（min {min} / max {max}）：需要两者都是有限数且 min < max"
            ))
        }
        GeneratorConfig::RandomDecimal { min, max, .. }
            if !min.is_finite() || !max.is_finite() || min >= max =>
        {
            Some(format!(
                "的随机小数区间不是有效区间（min {min} / max {max}）：需要两者都是有限数且 min < max"
            ))
        }
        GeneratorConfig::Words { min, max }
        | GeneratorConfig::Sentence { min, max }
        | GeneratorConfig::Sentences { min, max }
        | GeneratorConfig::Password { min, max }
            if min >= max =>
        {
            Some(format!(
                "的取值区间是空的（min {min} ≥ max {max}）：需要 min < max（区间是半开区间）"
            ))
        }

        // ===== 分布族：参数越界会让抽样退化成常量或 NaN =====
        GeneratorConfig::Poisson { lambda } | GeneratorConfig::Exponential { lambda }
            if !lambda.is_finite() || *lambda <= 0.0 =>
        {
            Some(format!(
                "的「强度 λ」需为大于 0 的有限数（当前 {lambda}）：它决定事件到达速率，越小事件越稀"
            ))
        }
        GeneratorConfig::Pareto { scale_value, .. }
            if !scale_value.is_finite() || *scale_value <= 0.0 =>
        {
            Some(format!(
                "的「尺度（最小值）」需为大于 0 的有限数（当前 {scale_value}）：帕累托分布的取值不会低于它"
            ))
        }
        GeneratorConfig::Pareto { alpha, .. } if !alpha.is_finite() || *alpha <= 0.0 => {
            Some(format!(
                "的「形状参数 α」需为大于 0 的有限数（当前 {alpha}）：α 越小尾巴越重，为 0 时算不出取值"
            ))
        }
        GeneratorConfig::Beta { alpha, beta }
            if !alpha.is_finite() || !beta.is_finite() || *alpha <= 0.0 || *beta <= 0.0 =>
        {
            Some(format!(
                "的「形状参数 α / β」需为大于 0 的有限数（当前 α {alpha} / β {beta}）：Beta 分布取值落在 0~1 之间"
            ))
        }
        GeneratorConfig::Binomial { trials, .. } if *trials == 0 => {
            Some("的「试验次数 n」需大于 0：n 为 0 时整列都是同一个值".to_string())
        }
        GeneratorConfig::Binomial { probability, .. }
            if !probability.is_finite() || *probability < 0.0 || *probability > 1.0 =>
        {
            Some(format!(
                "的「概率 p」需落在 0~1 之间（当前 {probability}）：p 是单次试验成功的概率"
            ))
        }
        GeneratorConfig::TimeSeries {
            start,
            trend,
            amplitude,
            noise,
            ..
        } if !start.is_finite()
            || !trend.is_finite()
            || !amplitude.is_finite()
            || !noise.is_finite() =>
        {
            Some(format!(
                "的「起始值 / 趋势 / 周期振幅 / 噪声强度」需均为有限数（当前 {start} / {trend} / {amplitude} / {noise}）：NaN 或无穷会让整列取值失去意义"
            ))
        }

        // ===== 日期时间类：fake 按「分钟差」取随机偏移，差 ≤ 0 就是空区间 =====
        GeneratorConfig::DateTime { min, max } => datetime_range_problem(min, max),
        GeneratorConfig::Date { min, max } => date_range_problem(min, max),

        // ===== 工作日历（列级参数）：勾了「仅工作日」才校验，没勾时留个值不该拦住生成 =====
        // 必须排在下面那条全匹配的 `DateTimeBetween { .. }` 之前，否则永远走不到
        GeneratorConfig::DateTimeBetween {
            workdays_only,
            work_week,
            skip_dates,
            work_dates,
            ..
        } if *workdays_only => work_calendar_problem(work_week, skip_dates, work_dates),
        GeneratorConfig::SequentialDate {
            workdays_only,
            step_seconds,
            work_week,
            skip_dates,
            work_dates,
            ..
        } if *workdays_only => work_days_step_problem(*step_seconds)
            .or_else(|| work_calendar_problem(work_week, skip_dates, work_dates)),
        GeneratorConfig::SequentialDateWithGaps {
            workdays_only,
            step_seconds,
            work_week,
            skip_dates,
            work_dates,
            ..
        } if *workdays_only => work_days_step_problem(*step_seconds)
            .or_else(|| work_calendar_problem(work_week, skip_dates, work_dates)),
        GeneratorConfig::DateTimeBetween {
            start: min,
            end: max,
            ..
        } => datetime_range_problem(min, max),

        _ => None,
    }
}

/// 工作日历里两个日期列表各自最多多少条。
///
/// 不是存储限制而是**生成期预算**：「按工作日推进」要对每个值数一遍区间内的工作日，
/// 列表越长逐行判定越久（一年节假日 ≈ 20 条，366 条已是一年逐日列满的量级）。
const MAX_CALENDAR_DATES: usize = 366;

/// 工作日历（工作周掩码 + 跳过日期 + 上班日期）的合法性。
///
/// 三个日期生成器共用同一套规则；非法值必须拦在生成前——掩码非法会让日历静默退到周一~周五，
/// 日期串写错会让那一天静默不生效，两者用户都看不出来。
fn work_calendar_problem(
    work_week: &str,
    skip_dates: &[String],
    work_dates: &[String],
) -> Option<String> {
    let bytes = work_week.as_bytes();
    if bytes.len() != 7 || !bytes.iter().all(|b| *b == b'0' || *b == b'1') {
        return Some(format!(
            "的「工作周（周一~周日，1 上班）」需是 7 位 0/1（当前「{work_week}」）：如 1111100 = 周一~周五上班"
        ));
    }
    if !bytes.iter().any(|b| *b == b'1') {
        return Some(
            "的「工作周（周一~周日，1 上班）」全为 0：至少要让一天上班，否则整列无值可放"
                .to_string(),
        );
    }
    for (label, list) in [
        ("跳过日期（节假日）", skip_dates),
        ("上班日期（调休）", work_dates),
    ] {
        if list.len() > MAX_CALENDAR_DATES {
            return Some(format!(
                "的「{label}」最多 {MAX_CALENDAR_DATES} 条（当前 {} 条）：一年 20 条上下就够",
                list.len()
            ));
        }
        // 日期必须是**规范 10 位**：`2026-1-1` 也能被 chrono 解析，但与生成侧按字节比较的
        // 日历列表对不上，会静默失效——所以要求「解析回来与输入逐字相同」
        if let Some(bad) = list.iter().find(|d| {
            chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d")
                .map(|parsed| parsed.format("%Y-%m-%d").to_string() != **d)
                .unwrap_or(true)
        }) {
            return Some(format!(
                "的「{label}」里「{bad}」不是日期：需要 10 位的 `YYYY-MM-DD`（一行一个）"
            ));
        }
    }
    None
}

/// 「按工作日推进」要求步长是**整整天的正数**：半天 / 负数在「工作日」这个刻度上没有意义。
fn work_days_step_problem(step_seconds: i32) -> Option<String> {
    if step_seconds < 86_400 || step_seconds % 86_400 != 0 {
        return Some(format!(
            "的「步长（秒）」勾选「仅工作日」时需是不小于 86400 的整天数（当前 {step_seconds} 秒）：如 86400 = 每个工作日一行"
        ));
    }
    None
}

/// `DateTime` / `DateTimeBetween` 的区间问题：解析得出的两界差值必须**至少一分钟**。
///
/// 解析不出来的串不在这里拦：生成器自己会回退到默认窗口（并记 `tracing::warn!`）。
fn datetime_range_problem(min: &str, max: &str) -> Option<String> {
    let parse = |text: &str| {
        chrono::DateTime::parse_from_rfc3339(text)
            .ok()
            .map(|d| d.to_utc())
    };
    let (Some(start), Some(end)) = (parse(min), parse(max)) else {
        return None;
    };
    ((end - start).num_minutes() <= 0)
        .then(|| format!("的时间区间不足一分钟（{min} ~ {max}）：需要 max 比 min 晚一分钟以上"))
}

/// `Date` 的区间问题：两界都能解析时，`max` 不能早于 `min`（同一天可以）。
fn date_range_problem(min: &str, max: &str) -> Option<String> {
    let parse = |text: &str| chrono::NaiveDate::parse_from_str(text, "%Y-%m-%d").ok();
    let (Some(start), Some(end)) = (parse(min), parse(max)) else {
        return None;
    };
    (end < start).then(|| format!("的日期区间反向（{min} ~ {max}）：把 max 改到 min 之后"))
}

fn sanitize_table_name(name: &str) -> String {
    let safe = sanitize_identifier(name).to_lowercase();
    if safe.is_empty() {
        format!(
            "auto_table_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        )
    } else {
        safe
    }
}

/// DuckDB 值 → SQL 字面量（导出 `.sql` 用）。
///
/// 覆盖范围必须 ⊇ mock 能产出的列类型（`ColumnDataType::to_duckdb_type` 的取值集合）：
/// 时间戳 / 日期 / 十进制 / 大整数 / 二进制一个都不能漏——漏掉的会被**静默**写成 `NULL`，
/// 导出的脚本就悄悄丢了这些列的数据（v1 遗留：这里原本只列了 9 个变体，DateTime / DECIMAL 全变 NULL）。
fn value_to_sql_literal(val: &duckdb::types::Value) -> String {
    use duckdb::types::Value;
    match val {
        Value::Null => "NULL".to_string(),
        Value::Boolean(b) => b.to_string(),
        Value::TinyInt(i) => i.to_string(),
        Value::SmallInt(i) => i.to_string(),
        Value::Int(i) => i.to_string(),
        Value::BigInt(i) => i.to_string(),
        Value::HugeInt(i) => i.to_string(),
        Value::UHugeInt(i) => i.to_string(),
        Value::UTinyInt(i) => i.to_string(),
        Value::USmallInt(i) => i.to_string(),
        Value::UInt(i) => i.to_string(),
        Value::UBigInt(i) => i.to_string(),
        Value::Float(f) => f.to_string(),
        Value::Double(f) => f.to_string(),
        // 十进制按**数字字面量**写（`Decimal` 的 Display 带小数点，不加引号）
        Value::Decimal(d) => d.to_string(),
        Value::Text(s) => quote_sql_text(s),
        Value::Enum(s) => quote_sql_text(s),
        // 时间类加类型前缀：目标库不会把它当字符串列
        Value::Timestamp(unit, raw) => format!(
            "TIMESTAMP '{}'",
            engine::duckdb::value_text::timestamp_text(*unit, *raw)
        ),
        Value::Date32(days) => {
            format!("DATE '{}'", engine::duckdb::value_text::date_text(*days))
        }
        Value::Time64(unit, raw) => format!(
            "TIME '{}'",
            engine::duckdb::value_text::time_text(*unit, *raw)
        ),
        Value::Interval {
            months,
            days,
            nanos,
        } => format!(
            "INTERVAL '{}'",
            engine::duckdb::value_text::interval_text(*months, *days, *nanos)
        ),
        Value::Blob(bytes) | Value::Geometry(bytes) => {
            format!("'{}'::BLOB", engine::duckdb::value_text::blob_hex(bytes))
        }
        // mock 产不出的容器 / 联合类型：不再静默——真出现了要看得见
        other => {
            tracing::warn!(
                "Mock: 导出 SQL 时遇到未覆盖的 DuckDB 值类型（{:?}），已写 NULL",
                std::mem::discriminant(other)
            );
            "NULL".to_string()
        }
    }
}

fn quote_sql_text(text: &str) -> String {
    format!("'{}'", text.replace('\'', "''"))
}

impl MockEngine {
    /// 将临时表保存到用户草稿本
    ///
    /// 按 `mock_{表名}_{时间戳}.{ext}` 写入给定目录，返回**落盘文件路径**
    /// （供面板显示具体位置）。
    pub fn save_to_scratchpad(
        temp_table_name: &str,
        format: &MockExportFormat,
        scratchpad_dir: &str,
    ) -> MockResult<String> {
        let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S");
        let base_name = temp_table_name.trim_start_matches(TEMP_MOCK_PREFIX);
        let ext = match format {
            MockExportFormat::Csv => "csv",
            MockExportFormat::Parquet => "parquet",
            MockExportFormat::Xlsx => "xlsx",
            MockExportFormat::SqlInsert => "sql",
            MockExportFormat::Table => "duckdb",
        };
        let file_name = format!("mock_{}_{}.{}", base_name, timestamp, ext);
        let output_path = format!("{}/{}", scratchpad_dir, file_name);

        Self::export(temp_table_name, format, Some(&output_path), None)?;
        Ok(output_path)
    }

    /// 将临时表持久化为项目资产
    ///
    /// 以新名称将临时表转为 DuckDB 持久化双挂载表（内存+文件），
    /// 返回 `(表名, 行数, 列数)`。
    pub fn persist_as_asset(
        temp_table_name: &str,
        new_name: &str,
    ) -> MockResult<(String, i64, i32)> {
        let db = Self::get_db()?;
        let conn = Self::get_conn(&db)?;

        let safe_name = sanitize_table_name(new_name);

        // 第二个参数是源**表名**（不是 SELECT 语句），见 `SqlEngine::build_create_table_as_select`。
        let create_sql = SqlEngine::build_create_table_as_select(&safe_name, temp_table_name);
        conn.execute_batch(&create_sql)?;

        let count_sql = SqlEngine::build_select(&safe_name, &["COUNT(*)"], None);
        let row_count: i64 = conn.query_row(&count_sql, [], |row| row.get(0))?;

        let column_count: i32;
        {
            let desc_sql = SqlEngine::build_select_all(&safe_name, Some(0));
            let mut stmt_desc = conn.prepare(&desc_sql)?;
            let _rows = stmt_desc.query([])?;
            column_count = stmt_desc.column_count() as i32;
        }

        let drop_sql = SqlEngine::build_drop_table(temp_table_name, true);
        conn.execute_batch(&drop_sql)?;

        Ok((safe_name, row_count, column_count))
    }
}

impl MockEngine {
    /// 解析场景模板里的**表间引用**，算出每条引用目标的取值域（生成前校验 + 采样依据）。
    ///
    /// 规则（全部可在生成前判定，不需要读任何已有数据）：
    ///
    /// 1. 父表必须在**同一个模板**里（跨模板引用不成立，那是别的项目/别的库的事了）；
    /// 2. 父列必须存在，且是 `AutoIncrement`——域 = `[start, start + step×(行数-1)]`；
    ///    非自增主键（uuid / 随机）算不出域，直接拒绝而不是去读已落地的数据。
    ///
    /// **不要求父表先于子表**：域由模板参数算出，与「哪张表先跑」无关——
    /// 引用自身（自关联）或引用后面才生成的表都是合法的，跑完整个场景两张表都在。
    ///
    /// 返回的域清单已按 `(父表, 父列)` 去重；面板也调它做**提交前**校验。
    pub fn resolve_reference_domains(
        template: &ScenarioTemplate,
    ) -> MockResult<Vec<ReferenceDomain>> {
        let mut domains: Vec<ReferenceDomain> = Vec::new();
        for table in &template.tables {
            for col in &table.columns {
                let Some(dep) = &col.dependency else { continue };
                let parent_name = dep.ref_table.as_str();
                let parent_column = dep.ref_column.as_str();
                if parent_name.is_empty() || parent_column.is_empty() {
                    return Err(MockError::InvalidColumn(format!(
                        "表 '{}' 的列 '{}' 声明了引用但没写清父表 / 父列",
                        table.name, col.name
                    )));
                }
                let Some(parent_idx) = template
                    .tables
                    .iter()
                    .position(|t| t.name == parent_name)
                else {
                    return Err(MockError::InvalidColumn(format!(
                        "表 '{}' 的列 '{}' 引用了模板里没有的表 '{parent_name}'",
                        table.name, col.name
                    )));
                };
                let parent = &template.tables[parent_idx];
                let Some(parent_col) = parent.columns.iter().find(|c| c.name == parent_column)
                else {
                    return Err(MockError::InvalidColumn(format!(
                        "表 '{}' 的列 '{}' 引用了 '{parent_name}.{parent_column}'，\
                         但父表里没有这一列",
                        table.name, col.name
                    )));
                };
                let GeneratorConfig::AutoIncrement { start, step } = parent_col.generator else {
                    return Err(MockError::InvalidColumn(format!(
                        "父表 '{parent_name}' 的列 '{parent_column}' 不是自增生成，算不出取值域——\
                         引用目标目前只支持自增主键（不读已生成的数据）"
                    )));
                };
                if parent.row_count == 0 {
                    return Err(MockError::InvalidColumn(format!(
                        "父表 '{parent_name}' 的行数为 0，没有可引用的主键值"
                    )));
                }
                if domains
                    .iter()
                    .any(|d| d.table == parent_name && d.column == parent_column)
                {
                    continue;
                }
                domains.push(ReferenceDomain {
                    table: parent_name.to_string(),
                    column: parent_column.to_string(),
                    first: i64::from(start),
                    step: i64::from(step),
                    count: parent.row_count,
                });
            }
        }
        Ok(domains)
    }

    /// 从域清单里找某条引用对应的父表取值域（不存在 → `None`，由调用方转成可读错误）。
    fn domain_for<'a>(
        dep: &ColumnDependency,
        domains: &'a [ReferenceDomain],
    ) -> Option<&'a ReferenceDomain> {
        let table = dep.ref_table.as_str();
        let column = dep.ref_column.as_str();
        domains
            .iter()
            .find(|d| d.table == table && d.column == column)
    }

    /// 多表场景批量生成
    ///
    /// 根据场景模板（ScenarioTemplate）一次性生成所有关联表。
    /// 每张表独立生成，支持进度回调和取消检查。
    /// 模板里声明了**表间引用**的列（列上带 `dependency`）从**父表主键域**取值，
    /// 域由父列的自增参数与行数算出——不读任何已落地的数据（见 [`Self::resolve_reference_domains`]）。
    /// 返回 `MockScenarioResult` 包含每张表的生成结果摘要。
    pub async fn generate_scenario(
        template: &ScenarioTemplate,
        on_table_progress: impl Fn(usize, usize) + Send + 'static,
    ) -> MockResult<MockScenarioResult> {
        let start = Instant::now();
        let mut table_results = Vec::new();
        let mut total_rows = 0u32;
        // 一次生成共用一份域清单：父表先于子表，因此下游能直接查
        let domains = Self::resolve_reference_domains(template)?;

        for (idx, table) in template.tables.iter().enumerate() {
            if Self::is_cancelled() {
                return Err(MockError::Generation("场景生成已取消".to_string()));
            }

            let config = MockConfig {
                table_name: table.name.clone(),
                row_count: table.row_count,
                seed: None,
                locale: match template.locale.as_str() {
                    "zh_cn" => Locale::ZhCn,
                    "en" | "en_us" => Locale::En,
                    "ja_jp" => Locale::JaJp,
                    _ => Locale::ZhCn,
                },
                columns: table.columns.clone(),
            };

            let result = Self::generate_table(config, &domains, |_, _| {}).await?;
            total_rows += result.row_count;

            table_results.push(MockScenarioTableResult {
                table_name: result.table_name.clone(),
                temp_table_name: result.temp_table_name,
                row_count: result.row_count,
                columns: result.columns,
                elapsed_ms: result.elapsed_ms,
            });

            on_table_progress(idx + 1, template.tables.len());
        }

        let total_elapsed_ms = start.elapsed().as_millis() as u32;

        Ok(MockScenarioResult {
            template_id: template.id.clone(),
            template_name: template.name.clone(),
            table_count: template.tables.len() as u32,
            total_rows,
            total_elapsed_ms,
            tables: table_results,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine::CoreError;
    use crate::generators::generate_cell;
    use crate::models::{ColumnDataType, GeneratorConfig};
    use crate::models::Locale;

    #[test]
    fn test_sanitize_table_name_alphanumeric() {
        assert_eq!(sanitize_table_name("hello_world"), "hello_world");
    }

    #[test]
    fn test_sanitize_table_name_with_spaces() {
        assert_eq!(sanitize_table_name("my table"), "my_table");
    }

    #[test]
    fn test_sanitize_table_name_special_chars() {
        assert_eq!(sanitize_table_name("user-data@2024"), "user_data_2024");
    }

    #[test]
    fn test_build_create_table_ddl_single_column() {
        let cols = vec![ColumnDef {
            name: "id".to_string(),
            data_type: ColumnDataType::Integer,
            generator: GeneratorConfig::AutoIncrement { start: 1, step: 1 },
            nullable_ratio: 0.0,
            unique: true,
            dependency: None,
        }];
        let ddl = MockEngine::build_create_table_ddl("users", &cols).expect("列名合法");
        assert_eq!(ddl, "CREATE TABLE \"users\" (id INT UNIQUE NOT NULL)");
    }

    #[test]
    fn test_build_create_table_ddl_multi_column() {
        let cols = vec![
            ColumnDef {
                name: "id".to_string(),
                data_type: ColumnDataType::Integer,
                generator: GeneratorConfig::AutoIncrement { start: 1, step: 1 },
                nullable_ratio: 0.0,
                unique: true,
                dependency: None,
            },
            ColumnDef {
                name: "name".to_string(),
                data_type: ColumnDataType::Varchar { length: Some(100) },
                generator: GeneratorConfig::Name,
                nullable_ratio: 0.0,
                unique: false,
                dependency: None,
            },
        ];
        let ddl = MockEngine::build_create_table_ddl("users", &cols).expect("列名合法");
        assert_eq!(
            ddl,
            "CREATE TABLE \"users\" (id INT UNIQUE NOT NULL, name VARCHAR(100) NOT NULL)"
        );
    }

    /// 列名要按 `sanitize_identifier` 落到 DDL 上：插入值时的列清单是同一算法得出的。
    ///
    /// 不这样做的后果是实测过的：`Order Date` 会直接产出非法 SQL
    /// （`CREATE TABLE ... (Order Date VARCHAR …)`）——DuckDB 报 `Parser Error`，
    /// 而用户完全看不出是「列名带空格」造成的。
    #[test]
    fn create_table_ddl_sanitizes_column_names() {
        let cols = vec![
            ColumnDef {
                name: "Order Date".to_string(),
                data_type: ColumnDataType::Date,
                generator: GeneratorConfig::Date {
                    min: "2024-01-01".to_string(),
                    max: "2024-12-31".to_string(),
                },
                nullable_ratio: 0.0,
                unique: false,
                dependency: None,
            },
            ColumnDef {
                name: "#序号".to_string(),
                data_type: ColumnDataType::Integer,
                generator: GeneratorConfig::RandomInt { min: 1, max: 10 },
                nullable_ratio: 0.0,
                unique: false,
                dependency: None,
            },
        ];
        let ddl = MockEngine::build_create_table_ddl("probe_space", &cols).expect("能建表");
        assert!(ddl.contains("Order_Date"), "{ddl}");
        assert!(ddl.contains("序号"), "{ddl}");
        assert!(!ddl.contains("Order Date"), "{ddl}");
    }

    /// 净化后为空 / 重名要在建表前报可读错误，而不是把 DuckDB 的原始报错扔给用户。
    #[test]
    fn create_table_ddl_reports_unusable_column_names() {
        let blank = vec![ColumnDef {
            name: "***".to_string(),
            data_type: ColumnDataType::Integer,
            generator: GeneratorConfig::RandomInt { min: 1, max: 10 },
            nullable_ratio: 0.0,
            unique: false,
            dependency: None,
        }];
        let reason = MockEngine::build_create_table_ddl("probe_blank", &blank)
            .expect_err("全符号列名要拒")
            .to_string();
        assert!(reason.contains("去掉符号后为空"), "{reason}");

        let duplicated = vec![
            ColumnDef {
                name: "user id".to_string(),
                data_type: ColumnDataType::Integer,
                generator: GeneratorConfig::RandomInt { min: 1, max: 10 },
                nullable_ratio: 0.0,
                unique: false,
                dependency: None,
            },
            ColumnDef {
                name: "user_id".to_string(),
                data_type: ColumnDataType::Integer,
                generator: GeneratorConfig::RandomInt { min: 1, max: 10 },
                nullable_ratio: 0.0,
                unique: false,
                dependency: None,
            },
        ];
        let reason = MockEngine::build_create_table_ddl("probe_dup", &duplicated)
            .expect_err("净化后重名要拒")
            .to_string();
        assert!(reason.contains("同名"), "{reason}");
    }

    #[test]
    fn test_generate_cell_auto_increment() {
        let mut rng = StdRng::seed_from_u64(42);
        let generator = GeneratorConfig::AutoIncrement {
            start: 100,
            step: 5,
        };
        let val = generate_cell(&generator, &mut rng, 3, &Locale::ZhCn);
        assert_eq!(val, "115");
    }

    #[test]
    fn test_generate_cell_random_int_range() -> Result<(), CoreError> {
        let mut rng = StdRng::seed_from_u64(42);
        let generator = GeneratorConfig::RandomInt { min: 10, max: 20 };
        let val: i64 = generate_cell(&generator, &mut rng, 0, &Locale::ZhCn)
            .parse()
            .map_err(|e| CoreError::from(format!("parse int error: {}", e)))?;
        assert!((10..=20).contains(&val));
        Ok(())
    }

    #[test]
    fn test_generate_cell_constant() {
        let mut rng = StdRng::seed_from_u64(42);
        let generator = GeneratorConfig::Constant {
            value: "hello".to_string(),
        };
        let val = generate_cell(&generator, &mut rng, 0, &Locale::ZhCn);
        assert_eq!(val, "hello");
    }

    #[test]
    fn test_generate_cell_boolean() {
        let mut rng = StdRng::seed_from_u64(42);
        let generator = GeneratorConfig::Boolean { ratio: 100 };
        let val = generate_cell(&generator, &mut rng, 0, &Locale::ZhCn);
        assert_eq!(val, "true");
    }

    /// 「不能采样」的参数必须在生成前被拦——它们原本会在工作线程里 panic，
    /// 而 panic 发生在持有内存库连接锁期间，会把整个进程的 mock 生成打坏（实测）。
    #[test]
    fn generator_params_that_cannot_be_sampled_are_rejected() {
        // 反向区间：实测 `RandomInt { min: 10, max: 5 }` 直接 panic
        assert!(
            generator_param_problem(&GeneratorConfig::RandomInt { min: 10, max: 5 }).is_some()
        );
        // 半开区间：`min == max` 就是空区间（`Sentence { min: 1, max: 1 }` 实测过）
        assert!(generator_param_problem(&GeneratorConfig::Sentence { min: 1, max: 1 }).is_some());
        assert!(
            generator_param_problem(&GeneratorConfig::RandomFloat {
                min: 1.0,
                max: 1.0,
                precision: 2,
            })
            .is_some()
        );
        assert!(
            generator_param_problem(&GeneratorConfig::RandomDecimal {
                min: 0.0,
                max: 0.0,
                scale: 2,
            })
            .is_some()
        );
        assert!(generator_param_problem(&GeneratorConfig::Words { min: 3, max: 2 }).is_some());
        assert!(generator_param_problem(&GeneratorConfig::Password { min: 8, max: 8 }).is_some());
        // 权重含负数：总权重可能 ≤ 0，抽 `0.0..total` 就是空区间
        assert!(
            generator_param_problem(&GeneratorConfig::Weighted {
                choices: vec![("a".to_string(), 5.0), ("b".to_string(), -5.0)],
            })
            .is_some()
        );
        // 时间区间不足一分钟：fake 走 `(0..分钟差)`，差 ≤ 0 即空区间
        assert!(
            generator_param_problem(&GeneratorConfig::DateTime {
                min: "2024-01-01T00:00:00Z".to_string(),
                max: "2024-01-01T00:00:30Z".to_string(),
            })
            .is_some()
        );
        assert!(
            generator_param_problem(&GeneratorConfig::Date {
                min: "2024-12-31".to_string(),
                max: "2024-01-01".to_string(),
            })
            .is_some()
        );
        // 分布族：参数越界会让抽样退化成常量 / NaN（λ ≤ 0、α ≤ 0、p 越界）
        assert!(generator_param_problem(&GeneratorConfig::Poisson { lambda: 0.0 }).is_some());
        assert!(generator_param_problem(&GeneratorConfig::Poisson { lambda: -1.0 }).is_some());
        assert!(
            generator_param_problem(&GeneratorConfig::Exponential { lambda: f64::NAN }).is_some()
        );
        assert!(
            generator_param_problem(&GeneratorConfig::Pareto {
                scale_value: 0.0,
                alpha: 2.0,
            })
            .is_some()
        );
        assert!(
            generator_param_problem(&GeneratorConfig::Pareto {
                scale_value: 1.0,
                alpha: 0.0,
            })
            .is_some()
        );
        assert!(
            generator_param_problem(&GeneratorConfig::Beta {
                alpha: 0.0,
                beta: 2.0,
            })
            .is_some()
        );
        assert!(
            generator_param_problem(&GeneratorConfig::Binomial {
                trials: 0,
                probability: 0.5,
            })
            .is_some()
        );
        assert!(
            generator_param_problem(&GeneratorConfig::Binomial {
                trials: 10,
                probability: 1.5,
            })
            .is_some()
        );
        assert!(
            generator_param_problem(&GeneratorConfig::TimeSeries {
                start: 0.0,
                trend: f64::INFINITY,
                period: 24,
                amplitude: 1.0,
                noise: 1.0,
            })
            .is_some()
        );
        // 工作日历：掩码不是 7 位 0/1、全 0、日期串不规范、步长不是整天
        let calendar =
            |work_week: &str, skip: &[&str], work: &[&str]| GeneratorConfig::SequentialDate {
                start: "2024-01-01 00:00:00".to_string(),
                step_seconds: 86_400,
                workdays_only: true,
                work_week: work_week.to_string(),
                skip_dates: skip.iter().map(|d| (*d).to_string()).collect(),
                work_dates: work.iter().map(|d| (*d).to_string()).collect(),
            };
        assert!(generator_param_problem(&calendar("111100", &[], &[])).is_some());
        assert!(generator_param_problem(&calendar("1111a00", &[], &[])).is_some());
        assert!(generator_param_problem(&calendar("0000000", &[], &[])).is_some());
        // `2026-1-1` 能被 chrono 解析，但与生成侧的字节比较对不上，必须拦
        assert!(generator_param_problem(&calendar("1111100", &["2026-1-1"], &[])).is_some());
        assert!(generator_param_problem(&calendar("1111100", &[], &["2026-13-01"])).is_some());
        // 步长不是整天（半天 / 负数）在「工作日」刻度上没意义
        assert!(
            generator_param_problem(&GeneratorConfig::SequentialDate {
                start: "2024-01-01 00:00:00".to_string(),
                step_seconds: 43_200,
                workdays_only: true,
                work_week: "1111100".to_string(),
                skip_dates: Vec::new(),
                work_dates: Vec::new(),
            })
            .is_some()
        );
        // 日历列表超长（生成期预算）
        let too_many: Vec<String> = (0..400)
            .map(|i| format!("2026-01-{:02}", (i % 28) + 1))
            .collect();
        assert!(
            generator_param_problem(&GeneratorConfig::DateTimeBetween {
                start: "2024-01-01T00:00:00Z".to_string(),
                end: "2024-12-31T23:59:59Z".to_string(),
                workdays_only: true,
                work_hours_only: true,
                work_week: "1111100".to_string(),
                skip_dates: too_many,
                work_dates: Vec::new(),
            })
            .is_some()
        );
    }

    /// 合法参数不能被误拦：护栏过宽会让正常配置也生不出来。
    #[test]
    fn valid_generator_params_pass_the_guard() {
        for generator in [
            // `RandomInt` 是**闭**区间，min == max 合法
            GeneratorConfig::RandomInt { min: 5, max: 5 },
            GeneratorConfig::RandomInt { min: 1, max: 10 },
            GeneratorConfig::RandomFloat {
                min: 0.0,
                max: 1.0,
                precision: 2,
            },
            GeneratorConfig::Sentence { min: 1, max: 3 },
            GeneratorConfig::Weighted {
                choices: vec![("a".to_string(), 1.0), ("b".to_string(), 2.0)],
            },
            GeneratorConfig::DateTime {
                min: "2020-01-01T00:00:00Z".to_string(),
                max: "2025-12-31T23:59:59Z".to_string(),
            },
            // 同一天的日期区间合法（下界 00:00:00 ~ 上界 23:59:59）
            GeneratorConfig::Date {
                min: "2024-01-01".to_string(),
                max: "2024-01-01".to_string(),
            },
            // 解析不出来的时间串不由这里拦：生成器自己会回退到默认窗口
            GeneratorConfig::DateTime {
                min: "2024-01-01".to_string(),
                max: "2024-12-31".to_string(),
            },
            GeneratorConfig::AutoIncrement { start: 1, step: 1 },
            // 分布族边界：λ / α / p 取到端点值都合法
            GeneratorConfig::Poisson { lambda: 0.5 },
            GeneratorConfig::Exponential { lambda: 3.0 },
            GeneratorConfig::Pareto {
                scale_value: 1.0,
                alpha: 1.5,
            },
            GeneratorConfig::Beta {
                alpha: 2.0,
                beta: 5.0,
            },
            GeneratorConfig::Binomial {
                trials: 10,
                probability: 0.0,
            },
            GeneratorConfig::Binomial {
                trials: 10,
                probability: 1.0,
            },
            // `period` 为 0 表示「不叠加周期项」，是合法输入
            GeneratorConfig::TimeSeries {
                start: 0.0,
                trend: 0.0,
                period: 0,
                amplitude: 0.0,
                noise: 0.0,
            },
            // 工作日历：合法掩码（含单休、自定义工作周）+ 合法日期列表
            GeneratorConfig::SequentialDate {
                start: "2024-01-01 00:00:00".to_string(),
                step_seconds: 86_400,
                workdays_only: true,
                work_week: "1111100".to_string(),
                skip_dates: vec!["2024-10-01".to_string()],
                work_dates: vec!["2024-10-12".to_string()],
            },
            // 单休（周一~周六）也合法
            GeneratorConfig::SequentialDateWithGaps {
                start: "2024-01-01 00:00:00".to_string(),
                step_seconds: 172_800,
                miss_probability: 0.1,
                workdays_only: true,
                work_week: "1111110".to_string(),
                skip_dates: Vec::new(),
                work_dates: Vec::new(),
            },
            // 没勾「仅工作日」时，日历字段里留着旧值不该拦住生成
            GeneratorConfig::DateTimeBetween {
                start: "2024-01-01T00:00:00Z".to_string(),
                end: "2024-12-31T23:59:59Z".to_string(),
                workdays_only: false,
                work_hours_only: false,
                work_week: "111100".to_string(),
                skip_dates: vec!["2026-1-1".to_string()],
                work_dates: Vec::new(),
            },
        ] {
            assert!(generator_param_problem(&generator).is_none(), "{generator:?}");
        }
    }

    /// 时间 / 日期 / 十进制 / 大整数 / 二进制都要有字面量：
    /// 这几类曾经整片漏掉（`_ => "NULL"`），导出的 `.sql` 会静默丢数据。
    #[test]
    fn sql_literals_cover_every_mock_column_type() {
        use duckdb::types::{Decimal, TimeUnit, Value};

        assert_eq!(
            value_to_sql_literal(&Value::Timestamp(TimeUnit::Second, 1_704_164_645)),
            "TIMESTAMP '2024-01-02 03:04:05'"
        );
        assert_eq!(
            value_to_sql_literal(&Value::Date32(19_724)),
            "DATE '2024-01-02'"
        );
        assert_eq!(
            value_to_sql_literal(&Value::Time64(TimeUnit::Second, 5)),
            "TIME '00:00:05'"
        );
        assert_eq!(
            value_to_sql_literal(&Value::Decimal(Decimal::new(4, 2, 1234).expect("decimal"))),
            "12.34"
        );
        assert_eq!(
            value_to_sql_literal(&Value::HugeInt(i128::from(i64::MAX) + 1)),
            "9223372036854775808"
        );
        assert_eq!(
            value_to_sql_literal(&Value::Blob(vec![0x41, 0x0A])),
            "'\\x41\\x0A'::BLOB"
        );
        assert_eq!(
            value_to_sql_literal(&Value::Text("it's".to_string())),
            "'it''s'"
        );
        assert_eq!(
            value_to_sql_literal(&Value::Interval {
                months: 1,
                days: 0,
                nanos: 3_600_000_000_000,
            }),
            "INTERVAL '1 months 01:00:00'"
        );
    }
}

//! Mock 数据生成装配层（M7）——把 mock crate 的面板意图翻译成真实读写。
//!
//! # 边界
//!
//! 本模块是 **M5 装配层**：`crates/mock/src/mock_view.rs` 的面板只认 mock crate 的类型
//! （[`MockHost`](mock::mock_view::MockHost) 注入的能力），「从哪张表读结构、往哪个库写」
//! 由本模块决定。列类型字符串解析的唯一权威是 `mock::parse_data_type`（不得在本层再写一份）。
//!
//! # 写入语义
//!
//! | 动作 | 目标 | 语义 |
//! | --- | --- | --- |
//! | `generate` | 内存临时表（DuckDB `temp_mock_*`） | **不写库**：只产数据 + 预览 |
//! | `persist_table` | 分析库 | **新建**表；已存在则 `Err`（引导改用「追加」） |
//! | `append_table` | 分析库既有表 | 保留既有数据，新行接在后面；主键自增接续表内行数 |
//! | `export_file` | 调用方指定路径 | CSV / Parquet / Xlsx / SQL INSERT |
//! | `save_scratchpad` | `{项目}/mock/` | 时间戳命名；无项目报错 |
//!
//! `*_at` 变体接受显式路径：集成测试用，也是「项目作用域分析库」将来的接入点
//! （生产入口恒取 [`analytics_db_path`]）。
//!
//! # 已知取舍
//!
//! 落库走「临时表 → INSERT 文本 → 分析库连接 `execute_batch`」：mock 的临时表在**进程级内存库**
//! 里，分析库是文件库，跨库不能直接 `CTAS`。`MockEngine::insert_statements` 已把「读全量 + 拼 INSERT」
//! 收在 mock crate 内（不再落地临时文件），后续可换成 DuckDB `ATTACH` 直写（见架构 §9-I3）。

use std::path::{Path, PathBuf};

use engine::sql::{ColumnDefInfo, SqlEngine};
use mock::mock_view::{
    MockColumnSpec, MockDraft, MockGenInfo, MockPreview, SchemaRequest, SchemaSource,
};
use mock::models::{ColumnDef, GeneratorConfig, MockConfig, MockExportFormat};
use mock::{MockEngine, parse_data_type, sanitize_identifier};

use crate::services::nav_runtime;

/// 分析库（DuckDB）路径：当前与 SQL 执行入口同口径（全局分析库）。
///
/// 项目作用域分析库（`{项目}/.RSmeta/analytics.duckdb`）的切换属待拍板项，
/// 见 `docs/architecture/mock/mock-prototype-design.md` §8-（3）。
pub fn analytics_db_path() -> PathBuf {
    crate::services::workspace_loader::global_analysis_db_path()
}

/// 打开分析库连接（读写）。
fn open_analysis_db(path: &Path) -> Result<duckdb::Connection, String> {
    duckdb::Connection::open(path).map_err(|e| format!("打开分析库失败: {e}"))
}

/// 分析库 `main` schema 下的表名（排序）。
fn analysis_tables(conn: &duckdb::Connection) -> Result<Vec<String>, String> {
    let sql = SqlEngine::build_select(
        "information_schema.tables",
        &["table_name", "table_schema"],
        None,
    );
    let mut stmt = conn
        .prepare(&sql)
        .map_err(|e| format!("读取表清单失败: {e}"))?;
    let mut names = Vec::new();
    {
        let mut rows = stmt
            .query([])
            .map_err(|e| format!("读取表清单失败: {e}"))?;
        while let Some(row) = rows.next().map_err(|e| format!("读取表清单失败: {e}"))? {
            let schema: String = row.get(1).unwrap_or_default();
            if schema == "main" {
                names.push(row.get::<usize, String>(0).unwrap_or_default());
            }
        }
    }
    names.sort();
    names.dedup();
    Ok(names)
}

/// 分析库中可作追加目标的分析表（读取失败返回空清单：面板显示「暂无表」）。
pub fn existing_tables() -> Vec<String> {
    existing_tables_at(&analytics_db_path())
}

/// [`existing_tables`] 的显式路径版本。
pub fn existing_tables_at(path: &Path) -> Vec<String> {
    match open_analysis_db(path) {
        Ok(conn) => analysis_tables(&conn).unwrap_or_default(),
        Err(_) => Vec::new(),
    }
}

/// 目标表现有行数（表不存在时报错，由调用方转成提示）。
fn table_row_count(conn: &duckdb::Connection, table: &str) -> Result<i64, String> {
    let sql = SqlEngine::build_select(table, &["COUNT(*)"], None);
    conn.query_row(&sql, [], |row| row.get(0))
        .map_err(|_| format!("分析库没有表 {table}"))
}

/// 目标表列名（表不存在时报错）。
fn table_columns(conn: &duckdb::Connection, table: &str) -> Result<Vec<String>, String> {
    let sql = SqlEngine::build_select_all(table, Some(0));
    let mut stmt = conn
        .prepare(&sql)
        .map_err(|_| format!("分析库没有表 {table}"))?;
    let _rows = stmt
        .query([])
        .map_err(|e| format!("读取表 {table} 结构失败: {e}"))?;
    Ok(stmt.column_names().iter().map(|c| c.to_string()).collect())
}

// ==================== 生成 ====================

/// 生成到内存临时表（面板 `MockHost::start_job` 的实现体，同步阻塞；UI 侧经 `services::mock_jobs` 放后台线程）。
///
/// `append_to` 给定时按目标表现有行数接续主键自增起点（「追加到既有表」用）。
pub fn generate(draft: &MockDraft, append_to: Option<&str>) -> Result<MockGenInfo, String> {
    generate_at(&analytics_db_path(), draft, append_to)
}

/// [`generate`] 的显式路径版本（集成测试 / 未来项目作用域分析库）。
pub fn generate_at(
    db_path: &Path,
    draft: &MockDraft,
    append_to: Option<&str>,
) -> Result<MockGenInfo, String> {
    generate_at_with_progress(db_path, draft, append_to, |_, _| {})
}

/// [`generate_at`] 的带进度版本：`on_progress(已完成批次, 总批次)` 由引擎按批回调。
///
/// 回调在工作线程上执行（`Fn + Send + 'static`），只能写共享原子量 / 锁保护的状态，
/// 不得回到 UI 线程。
pub fn generate_at_with_progress<F>(
    db_path: &Path,
    draft: &MockDraft,
    append_to: Option<&str>,
    on_progress: F,
) -> Result<MockGenInfo, String>
where
    F: Fn(usize, usize) + Send + 'static,
{
    if draft.columns.is_empty() {
        return Err("没有可用的列定义：请先导入源库结构或手工加列".to_string());
    }

    let mut columns: Vec<ColumnDef> = draft.columns.iter().map(|c| c.def.clone()).collect();
    if let Some(table) = append_to {
        let conn = open_analysis_db(db_path)?;
        let existing = table_row_count(&conn, table)?;
        for def in columns.iter_mut() {
            if let GeneratorConfig::AutoIncrement { step, .. } = def.generator {
                def.generator = GeneratorConfig::AutoIncrement {
                    start: (existing + 1).clamp(1, i32::MAX as i64) as i32,
                    step,
                };
            }
        }
    }

    let config = MockConfig {
        table_name: draft.table_name.clone(),
        row_count: draft.options.rows,
        seed: draft.options.seed,
        locale: draft.options.locale.clone(),
        columns,
    };

    let rt = nav_runtime::bridge_runtime()?;
    let result = rt
        .block_on(MockEngine::generate_with_progress(config, on_progress))
        .map_err(|e| format!("Mock 生成失败: {e}"))?;

    Ok(MockGenInfo {
        temp_table_name: result.temp_table_name,
        row_count: result.row_count,
        elapsed_ms: result.elapsed_ms,
        preview: flatten_preview(&result.preview),
    })
}

// ==================== 出口：落库 ====================

/// 建表 DDL（列名走 [`sanitize_identifier`]：与临时表列名同一算法）。
fn create_table_ddl(table: &str, columns: &[MockColumnSpec]) -> String {
    let infos: Vec<ColumnDefInfo> = columns
        .iter()
        .map(|c| ColumnDefInfo {
            name: sanitize_identifier(&c.def.name),
            data_type: c.def.data_type.to_duckdb_type(),
            unique: c.def.unique,
            nullable: c.def.nullable_ratio > 0.0,
        })
        .collect();
    SqlEngine::build_create_table(table, &infos, false)
}

/// 出口：在分析库**新建**表（已存在 → `Err`，面板据此引导改用「追加」）。
///
/// 写入失败时回滚刚建的表：不留下半成品空表。
pub fn persist_table(draft: &MockDraft, info: &MockGenInfo) -> Result<i64, String> {
    persist_table_at(&analytics_db_path(), draft, info)
}

/// [`persist_table`] 的显式路径版本。
pub fn persist_table_at(
    db_path: &Path,
    draft: &MockDraft,
    info: &MockGenInfo,
) -> Result<i64, String> {
    let name = draft.table_name.trim().to_string();
    if name.is_empty() {
        return Err("目标表名不能为空".to_string());
    }
    let conn = open_analysis_db(db_path)?;
    if analysis_tables(&conn)?.iter().any(|t| t == &name) {
        return Err(format!("分析库已存在表 {name}：请改用「追加到既有表」"));
    }

    let ddl = create_table_ddl(&name, &draft.columns);
    conn.execute_batch(&ddl)
        .map_err(|e| format!("在分析库建表失败: {e}"))?;

    let insert_text = MockEngine::insert_statements(&info.temp_table_name, Some(&name))
        .map_err(|e| format!("读取生成结果失败: {e}"))?;
    if let Err(e) = conn.execute_batch(&insert_text) {
        let _ = conn.execute_batch(&SqlEngine::build_drop_table(&name, true));
        return Err(format!("写入分析库失败（已回滚建表）: {e}"));
    }
    table_row_count(&conn, &name)
}

/// 出口：追加到分析库既有表（返回表内总行数）。
///
/// 列结构必须一致：缺失列直接报错，不让 DuckDB 的原始错误冒到界面上。
pub fn append_table(draft: &MockDraft, info: &MockGenInfo, table: &str) -> Result<i64, String> {
    append_table_at(&analytics_db_path(), draft, info, table)
}

/// [`append_table`] 的显式路径版本。
pub fn append_table_at(
    db_path: &Path,
    draft: &MockDraft,
    info: &MockGenInfo,
    table: &str,
) -> Result<i64, String> {
    let conn = open_analysis_db(db_path)?;
    let target_columns = table_columns(&conn, table)?;
    let missing: Vec<String> = draft
        .columns
        .iter()
        .map(|c| sanitize_identifier(&c.def.name))
        .filter(|name| !target_columns.iter().any(|t| t == name))
        .collect();
    if !missing.is_empty() {
        return Err(format!(
            "目标表 {table} 缺少列：{}（列结构需一致）",
            missing.join("、")
        ));
    }

    let insert_text = MockEngine::insert_statements(&info.temp_table_name, Some(table))
        .map_err(|e| format!("读取生成结果失败: {e}"))?;
    conn.execute_batch(&insert_text)
        .map_err(|e| format!("追加到 {table} 失败: {e}"))?;
    table_row_count(&conn, table)
}

// ==================== 出口：落盘 ====================

/// 出口：导出文件（CSV / Parquet / Xlsx / SQL INSERT）。
pub fn export_file(
    draft: &MockDraft,
    info: &MockGenInfo,
    format: &MockExportFormat,
    path: &str,
) -> Result<String, String> {
    MockEngine::export(
        &info.temp_table_name,
        format,
        Some(path),
        Some(&draft.table_name),
    )
    .map_err(|e| format!("导出失败: {e}"))
    .map(|_| format!("已导出：{path}"))
}

/// 出口：保存到草稿箱项目目录（`{项目}/mock/`）。
pub fn save_scratchpad(
    _draft: &MockDraft,
    info: &MockGenInfo,
    format: &MockExportFormat,
    project_root: Option<&Path>,
) -> Result<String, String> {
    let root =
        project_root.ok_or_else(|| "未打开项目：草稿箱不可用（请先打开或新建项目）".to_string())?;
    let dir = root.join("mock");
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建 {} 失败: {e}", dir.display()))?;
    let dir_text = dir.to_string_lossy().to_string();
    let path = MockEngine::save_to_scratchpad(&info.temp_table_name, format, &dir_text)
        .map_err(|e| format!("保存到草稿箱失败: {e}"))?;
    Ok(format!("已保存到草稿箱：{path}"))
}

/// 预览：Arrow 批次 → 字符串网格（视图层不依赖 Arrow）。
///
/// `read_preview` 只填 `batches`（`rows` 为空），取值必须经 `QueryResult::from_batches`。
fn flatten_preview(preview: &shared::models::QueryResult) -> MockPreview {
    let rows = shared::models::QueryResult::from_batches(
        preview.columns.clone(),
        preview.batches.clone(),
    )
    .rows;
    MockPreview {
        columns: preview.columns.clone(),
        rows: rows
            .iter()
            .map(|row| {
                row.iter()
                    .map(|value| match value {
                        shared::models::Value::Null => "NULL".to_string(),
                        other => other.to_string(),
                    })
                    .collect()
            })
            .collect(),
    }
}

// ==================== 来源与结构导入 ====================

/// 可导入结构的连接：来自工作台连接清单（`Shared::connections`）。
pub fn schema_sources(connections: &[crate::view::ConnectionItem]) -> Vec<SchemaSource> {
    connections
        .iter()
        .map(|c| SchemaSource {
            conn_id: c.id.clone(),
            label: c.name.clone(),
            catalog: c.database.clone().unwrap_or_default(),
            schema: c.schema.clone().unwrap_or_default(),
        })
        .collect()
}

/// 读源库某表的列结构 → mock 列规格（智能映射含置信度与示例值）。
///
/// cache-aside：**先查连接级 L2 缓存**（SQLite，同步、快），未命中再**实时内省**
/// （走 `MetadataService`，需要 tokio → 复用进程级桥接运行时），并尽力回写 L2。
pub fn import_columns(
    request: &SchemaRequest,
    project_root: Option<&str>,
) -> Result<Vec<MockColumnSpec>, String> {
    if request.conn_id.trim().is_empty() {
        return Err("请选择连接".to_string());
    }
    if request.table.trim().is_empty() {
        return Err("请填写表名".to_string());
    }
    let details = load_column_details(request, project_root)?;
    Ok(map_details(&details))
}

/// 读列详情（唯一实现）：L2 命中即返回，未命中实时内省并回写。
fn load_column_details(
    request: &SchemaRequest,
    project_root: Option<&str>,
) -> Result<Vec<engine::driver::traits::ColumnDetail>, String> {
    let schema = if request.schema.trim().is_empty() {
        "main"
    } else {
        request.schema.trim()
    };
    let catalog = request.catalog.trim();

    // 1) 连接级 L2 缓存（cache-aside 的读半边）
    let cache = database::cache::NavCache::open(&request.conn_id, project_root);
    let schema_id = cache.as_ref().and_then(|c| c.schema_id(catalog, schema));
    if let (Some(cache), Some(schema_id)) = (cache.as_ref(), schema_id) {
        if let Some(columns) = cache.columns(schema_id, &request.table) {
            return Ok(columns);
        }
    }

    // 2) 未命中：实时内省（驱动内省是 async，跑在进程级桥接运行时上）
    let rt = nav_runtime::bridge_runtime()?;
    let service =
        database::metadata_service::MetadataService::new(engine::get_connection_manager().clone());
    let columns = rt
        .block_on(service.list_columns(&request.conn_id, catalog, schema, &request.table))
        .map_err(|e| format!("读取源表结构失败: {e}"))?;
    if columns.is_empty() {
        return Err(format!(
            "源表 {catalog}.{schema}.{} 没有列信息（连接可能已断开，或表不存在）",
            request.table
        ));
    }

    // 3) 尽力回写 L2（写失败静默，不影响本次结果）
    if let (Some(cache), Some(schema_id)) = (cache.as_ref(), schema_id) {
        cache.put_columns(schema_id, &request.table, &columns);
    }
    Ok(columns)
}

/// 列详情 → mock 列规格（类型串唯一入口 + 智能映射）。
fn map_details(details: &[engine::driver::traits::ColumnDetail]) -> Vec<MockColumnSpec> {
    details
        .iter()
        .enumerate()
        .map(|(index, detail)| {
            let data_type = parse_data_type(&detail.data_type);
            let mapped = MockEngine::map_column(&detail.name, &detail.data_type).ok();
            let (generator, confidence, sample_value) = match mapped {
                Some(response) => (
                    response.generator,
                    response.confidence,
                    response.sample_value,
                ),
                // 映射失败（理论上不会）：退到类型兜底，置信度标 low
                None => (
                    GeneratorConfig::RandomInt { min: 0, max: 10000 },
                    "low".to_string(),
                    String::new(),
                ),
            };
            MockColumnSpec {
                id: index as u64 + 1,
                def: ColumnDef {
                    name: detail.name.clone(),
                    data_type,
                    generator,
                    nullable_ratio: if detail.nullable { 0.1 } else { 0.0 },
                    unique: detail.is_primary_key,
                    dependency: None,
                },
                confidence,
                sample_value,
            }
        })
        .collect()
}

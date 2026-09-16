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
//! 所有写入都落在**项目分析库** `{项目}/.RSmeta/analytics.duckdb`（未打开项目 → 拒绝，
//! 原因里写清替代路径）；要进全局分析库走 M6 资产库存档 / M5 草稿箱。
//!
//! | 动作 | 目标 | 语义 |
//! | --- | --- | --- |
//! | `generate_at_with_progress` | 内存临时表（DuckDB `temp_mock_*`） | **不写库**：只产数据 + 预览（按批回调进度）；`db_path` 为 `None` 也能跑（纯生成不碰库） |
//! | `generate_scenario_at` | 内存临时表（模板里的每张表一张） | **不写库**：按内置场景模板一次生成多张表，逐表补预览；进度按「张表」计 |
//! | `persist_table_at` | 项目分析库 | **新建**表；已存在则 `Err`（引导改用「追加」） |
//! | `append_table_at` | 项目分析库既有表 | 保留既有数据，新行接在后面；主键自增接续表内行数 |
//! | `export_file` | 调用方指定路径 | CSV / Parquet / Xlsx / SQL INSERT |
//! | `save_scratchpad` | `{项目}/mock/` | 时间戳命名；无项目报错 |
//!
//! 本层全是**同步**实现（阻塞当前线程）：生产入口是 `services::mock_jobs` 的任务种类，
//! 由它在工作线程上调用；`*_at` 变体接受显式路径——集成测试用，也是「任意项目根」的接入面
//! （生产路径由宿主桥从当前项目派生）。视图侧没有同步出口入口。
//!
//! # 已知取舍
//!
//! 落库走**跨库直写**（`MockEngine::write_temp_table_to_database`：`ATTACH` 分析库 →
//! （建表）→ `INSERT ... SELECT` → `DETACH`）：数据全程在 DuckDB 内部流动，不经 Rust 字符串。
//! 旧路径是「读全量 → 拼 INSERT 文本 → 目标库再解析」两跳，大行数下内存与耗时都不划算（架构 §9-I3）。
//! `insert_statements`（文本）保留给「导出 SQL 脚本」那个出口。

use std::path::{Path, PathBuf};

use engine::sql::{ColumnDefInfo, SqlEngine};
use mock::mock_view::{
    MockColumnSpec, MockDraft, MockGenInfo, MockPreview, SchemaRequest, SchemaSource,
};
use mock::models::{ColumnDef, GeneratorConfig, MockConfig, MockExportFormat};
use mock::{MockEngine, TempTableWriteMode, parse_data_type, sanitize_identifier};

use crate::services::nav_runtime;

/// 分析库（DuckDB）路径：**当前项目的项目库** `{项目}/.RSmeta/analytics.duckdb`。
///
/// # 为什么是项目级
///
/// mock 产出的是「这个项目造出来的测试数据」——它属于项目，不属于应用。
/// 早期实现恒取全局分析库（`{全局}/analytics.duckdb`），后果是：造的数据会掉进
/// 共享库并跨项目可见，既违背「窗口 = 项目」的隔离，也让「这份数据哪来的」无从追溯。
/// 要跨项目复用 / 进全局，走**资产库存档**（M6）或**草稿箱**（M5）的升级路径，
/// 而不是让 mock 直接写全局。
///
/// 未打开项目时返回 `None`：此时落库 / 追加不可用（面板与任务层都给可读原因），
/// 但**生成仍然可用**（只写进程内内存临时表，与库无关）。
pub fn analysis_db_path(project_root: Option<&Path>) -> Option<PathBuf> {
    project_root.map(engine::persistence::project_db::ProjectDatabaseManager::analysis_db_path)
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

/// 未打开项目时的统一理由（落库 / 追加 / 需要读既有表的生成都用它）。
///
/// 写全「替代路径」的原因：用户想往全局写时的正常做法是**存档升级**（M6），
/// 只说「请先打开项目」会让人以为功能被砍了。
const NO_PROJECT_DB: &str = "未打开项目：Mock 只写项目分析库（{项目}/.RSmeta/analytics.duckdb）\
——请先打开项目；要进全局分析库，用资产库存档（M6）或草稿箱（M5）升级";

/// 分析库中可作追加目标的分析表（读取失败返回空清单：面板显示「暂无表」）。
///
/// 未打开项目时也是空清单——真正的拒绝理由在提交任务时给（此处只负责取候选）。
pub fn existing_tables(project_root: Option<&Path>) -> Vec<String> {
    match analysis_db_path(project_root) {
        Some(path) => existing_tables_at(&path),
        None => Vec::new(),
    }
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
        .map_err(|_| format!("项目分析库没有表 {table}"))
}

/// 目标表列名（表不存在时报错）。
fn table_columns(conn: &duckdb::Connection, table: &str) -> Result<Vec<String>, String> {
    let sql = SqlEngine::build_select_all(table, Some(0));
    let mut stmt = conn
        .prepare(&sql)
        .map_err(|_| format!("项目分析库没有表 {table}"))?;
    let _rows = stmt
        .query([])
        .map_err(|e| format!("读取表 {table} 结构失败: {e}"))?;
    Ok(stmt.column_names().iter().map(|c| c.to_string()).collect())
}

// ==================== 生成 ====================

/// 生成到内存临时表（**不写库**）：`append_to` 给定时按目标表现有行数接续主键自增起点
/// （「追加到既有表」用，此时必须有项目分析库）。
///
/// `db_path` 是 `Option`：**纯生成不碰任何库**（内存临时表是进程级的，与项目无关），
/// 所以「未打开项目」也能生成预览；只有追加需要在生成期读目标表。
///
/// 落库 / 落盘是出口的职责（[`persist_table_at`] / [`append_table_at`] / [`export_file`]）。
pub fn generate_at(
    db_path: Option<&Path>,
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
    db_path: Option<&Path>,
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
        let Some(db_path) = db_path else {
            return Err(NO_PROJECT_DB.to_string());
        };
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
        // 留一份给结果：出口（落库建表 / 目标列校对）要凭它，不能再去读草稿
        columns: columns.clone(),
    };

    let rt = nav_runtime::bridge_runtime()?;
    let result = rt
        .block_on(MockEngine::generate_with_progress(config, on_progress))
        .map_err(|e| format!("Mock 生成失败: {e}"))?;

    Ok(MockGenInfo {
        table_name: result.table_name,
        temp_table_name: result.temp_table_name,
        columns,
        row_count: result.row_count,
        elapsed_ms: result.elapsed_ms,
        preview: flatten_preview(&result.preview),
    })
}

/// 预览行数上限：面板详情只展示前若干行（与「生成后立刻看几眼」的用途对齐）。
const PREVIEW_ROWS: usize = 20;

/// 场景模板：一次生成模板里的**全部表**（同样**不写库**，逐表产出内存临时表）。
///
/// 与 [`generate_at`] 的两点不同：
///
/// 1. 目标表 / 列 / 行数全部来自模板——**草稿不参与**（用户的多表选择与草稿配置互不干扰）；
/// 2. 引擎只回表级摘要（`MockScenarioTableResult` 不带预览），所以这里**逐表**再取一次
///    预览（`MockEngine::preview` 读同一张内存临时表），让面板的通用结果区对每张表都成立。
///
/// 进度回调按「张表」上报（`on_table_progress(已完成表数, 总表数)`），与单表生成的批次量纲不同。
pub fn generate_scenario_at<F>(
    template_id: &str,
    on_table_progress: F,
) -> Result<(String, Vec<MockGenInfo>), String>
where
    F: Fn(usize, usize) + Send + 'static,
{
    let template = mock::templates::get_template_by_id(template_id)
        .ok_or_else(|| format!("场景模板不存在: {template_id}"))?;
    if template.tables.is_empty() {
        return Err(format!("场景模板「{}」没有可生成的表", template.name));
    }

    let rt = nav_runtime::bridge_runtime()?;
    let scenario = rt
        .block_on(MockEngine::generate_scenario(&template, on_table_progress))
        .map_err(|e| format!("场景生成失败: {e}"))?;

    let mut tables = Vec::with_capacity(scenario.tables.len());
    for table in &scenario.tables {
        let preview = MockEngine::preview(&table.temp_table_name, PREVIEW_ROWS)
            .map_err(|e| format!("读取 {} 的预览失败: {e}", table.table_name))?;
        // 列定义取模板里那张表的（与引擎拿去建临时表的是同一份）：出口据此建同名同列的表
        let columns = template
            .tables
            .iter()
            .find(|t| t.name == table.table_name)
            .map(|t| t.columns.clone())
            .ok_or_else(|| format!("模板里找不到表 {}", table.table_name))?;
        tables.push(MockGenInfo {
            table_name: table.table_name.clone(),
            temp_table_name: table.temp_table_name.clone(),
            columns,
            row_count: table.row_count,
            elapsed_ms: table.elapsed_ms,
            preview: flatten_preview(&preview),
        });
    }

    Ok((scenario.template_name, tables))
}

// ==================== 出口：落库 ====================

/// 目标表的列定义（列名走 `sanitize_identifier`：与临时表列名同一算法，否则 `INSERT` 列清单对不上）。
fn column_def_infos(columns: &[ColumnDef]) -> Vec<ColumnDefInfo> {
    columns
        .iter()
        .map(|def| ColumnDefInfo {
            name: sanitize_identifier(&def.name),
            data_type: def.data_type.to_duckdb_type(),
            unique: def.unique,
            nullable: def.nullable_ratio > 0.0,
        })
        .collect()
}

/// 出口：在分析库**新建**表（已存在 → `Err`，面板据此引导改用「追加」）。
///
/// 目标表名与列定义都取自**结果自己**（[`MockGenInfo`]）：`info.table_name` 建表，
/// `info.columns` 出 DDL——场景模板的多张表因此能各自落到自己的表名下。
///
/// 写入失败时不留下半成品空表：建表与写行都在**同一个 ATTACH 会话**里（引擎侧失败即回滚建表）。
pub fn persist_table_at(db_path: &Path, info: &MockGenInfo) -> Result<i64, String> {
    let name = info.table_name.trim().to_string();
    if name.is_empty() {
        return Err("目标表名不能为空".to_string());
    }
    // 同名表检查用一条短连接（读一眼就关）：报错要早、要好懂；文件不存在时顺带建好
    {
        let conn = open_analysis_db(db_path)?;
        if analysis_tables(&conn)?.iter().any(|t| t == &name) {
            return Err(format!("项目分析库已存在表 {name}：请改用「追加到既有表」"));
        }
    }

    MockEngine::write_temp_table_to_database(
        db_path,
        &info.temp_table_name,
        &name,
        TempTableWriteMode::Create(column_def_infos(&info.columns)),
    )
    .map_err(|e| format!("写入项目分析库失败: {e}"))?;

    let conn = open_analysis_db(db_path)?;
    table_row_count(&conn, &name)
}

/// 出口：追加到分析库既有表（返回表内总行数）。
///
/// 列结构必须一致：缺失列直接报错，不让 DuckDB 的原始错误冒到界面上。
pub fn append_table_at(db_path: &Path, info: &MockGenInfo, table: &str) -> Result<i64, String> {
    {
        let conn = open_analysis_db(db_path)?;
        let target_columns = table_columns(&conn, table)?;
        let missing: Vec<String> = info
            .columns
            .iter()
            .map(|def| sanitize_identifier(&def.name))
            .filter(|name| !target_columns.iter().any(|t| t == name))
            .collect();
        if !missing.is_empty() {
            return Err(format!(
                "目标表 {table} 缺少列：{}（列结构需一致）",
                missing.join("、")
            ));
        }
    }

    MockEngine::write_temp_table_to_database(
        db_path,
        &info.temp_table_name,
        table,
        TempTableWriteMode::Append,
    )
    .map_err(|e| format!("追加到 {table} 失败: {e}"))?;

    let conn = open_analysis_db(db_path)?;
    table_row_count(&conn, table)
}

// ==================== 出口：落盘 ====================

/// 出口：导出文件（CSV / Parquet / Xlsx / SQL INSERT）。
///
/// 写进文件里的表名取自结果（SQL INSERT 的表名、Xlsx 的表头用），不是草稿。
pub fn export_file(
    info: &MockGenInfo,
    format: &MockExportFormat,
    path: &str,
) -> Result<String, String> {
    MockEngine::export(
        &info.temp_table_name,
        format,
        Some(path),
        Some(&info.table_name),
    )
    .map_err(|e| format!("导出失败: {e}"))
    .map(|_| format!("已导出：{path}"))
}

/// 出口：保存到草稿箱项目目录（`{项目}/mock/`）。
pub fn save_scratchpad(
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

/// 项目切换 / 关闭时清掉本进程的 mock 临时表（返回被删表名）。
///
/// 临时表建在 engine 的**进程级内存库**里，不随项目切换释放；宿主在切项目时调它，
/// 并让 mock 面板作废旧预览（见 `components::project_host::clear_mock_temp_tables`）。
/// 失败（拿不到内存库）不报错：清理失败最多是多占一点内存，不影响主流程。
pub fn clear_temp_tables() -> Vec<String> {
    mock::MockEngine::clear_temp_tables().unwrap_or_default()
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

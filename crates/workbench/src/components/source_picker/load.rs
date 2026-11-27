//! 数据访问：picker 需要的全部 I/O（内省、草稿枚举、资产库表、按层懒加载）。
//!
//! 单独成文件的原因很直接——这些是 `block_on` 与文件读，属于**事件路径**，
//! 与「状态 / 行模型 / 渲染」不是一类东西。放在 `mod.rs` 里会让那个文件永远瘦不下来
//! （初版 806 行、第一轮拆分后仍是 871 行，主要就是它撑的）。
//!
//! 口径沿用老实现，未改：
//! - 内省走 `NavigatorService`（有 L1/L2 缓存），同步 `block_on`，模态内点击粒度小可接受；
//!   先例是 `visible_databases_dialog`；
//! - 草稿箱按 `u32::MAX` 深度递归枚举，仅保留 `engine::file_reader_function` 认得的格式；
//! - 资产库直接查 `information_schema.tables` 且**必须**限定 `table_schema = 'main'`
//!   （否则 `temp` schema 的临时表会混进来）。

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use database::model::{NavFolder, NavNodeKind, NavPath};
use database::navigator_service::NavigatorService;
use scratchpad::ScratchpadEntry;
use gpui_kit::Window;

use super::db::{catalog_level, schema_rows, table_rows, CatalogLevel};
use super::{NodeId, PickerState};
use crate::panels::Shared;

/// 导航内省服务（同步 block_on：模态点击粒度小，有缓存时零 I/O）。
fn navigator(project_root: Option<PathBuf>) -> NavigatorService {
    NavigatorService::with_context(
        engine::get_connection_manager().clone(),
        project_root.map(|p| p.to_string_lossy().to_string()),
        false,
    )
}

/// 同步查某连接的 catalog 清单。
fn query_catalogs(conn_id: &str, project_root: Option<PathBuf>) -> Vec<String> {
    let svc = navigator(project_root);
    let rt = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(_) => return Vec::new(),
    };
    rt.block_on(async { svc.list_all_catalogs(conn_id).await.unwrap_or_default() })
}

/// 同步查某 catalog 的 schema 清单。
fn query_schemas(conn_id: &str, catalog: &str, project_root: Option<PathBuf>) -> Vec<String> {
    let svc = navigator(project_root);
    let rt = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(_) => return Vec::new(),
    };
    rt.block_on(async {
        svc.list_all_schemas(conn_id, catalog)
            .await
            .unwrap_or_default()
    })
}

/// 某 catalog / schema 下能分析的表与视图（**异步内核**：错误原话往上抛）。
///
/// 深度规则与导航一致：schema 为空串 = 该驱动无独立 schema 层，类别文件夹直接挂 catalog
/// 下；否则先展开 Schema 节点拿类别文件夹，再取表 / 视图节点。
async fn tables_under(
    svc: &NavigatorService,
    conn_id: &str,
    catalog: &str,
    schema: &str,
) -> Result<Vec<super::TableCandidate>, String> {
    let path = if schema.is_empty() {
        NavPath::Catalog {
            catalog: catalog.to_string(),
        }
    } else {
        NavPath::Schema {
            catalog: catalog.to_string(),
            schema: schema.to_string(),
        }
    };
    svc.load_children(conn_id, &path, None, false)
        .await
        .map_err(|error| error.to_string())?;
    let mut out = Vec::new();
    // 类别文件夹展开取表 / 视图节点（folder 路径的 schema 与展开路径一致）。
    // 某一类取不到不算失败（视图文件夹在部分驱动上就是空的），所以这里吞掉单类的错
    for folder in [NavFolder::Tables, NavFolder::Views] {
        let folder_path = NavPath::Folder {
            catalog: catalog.to_string(),
            schema: schema.to_string(),
            folder,
        };
        let Ok(children) = svc
            .load_children(conn_id, &folder_path, None, false)
            .await
        else {
            continue;
        };
        for node in children {
            let is_table = matches!(node.kind, NavNodeKind::Table { .. } | NavNodeKind::View);
            if is_table {
                out.push(super::TableCandidate {
                    name: node.name.clone(),
                    catalog: catalog.to_string(),
                    schema: schema.to_string(),
                });
            }
        }
    }
    Ok(out)
}

/// 同步查某 schema 下的表（含视图；对齐导航「类别文件夹 → Tables/Views」模型）。
///
/// `schema` 为空串 = 无独立 schema 层：catalog 直接承载类别文件夹。
fn query_tables(
    conn_id: &str,
    catalog: &str,
    schema: &str,
    project_root: Option<PathBuf>,
) -> Vec<super::TableCandidate> {
    let svc = navigator(project_root);
    let rt = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(_) => return Vec::new(),
    };
    rt.block_on(tables_under(&svc, conn_id, catalog, schema))
        .unwrap_or_default()
}

/// 草稿箱里 duckdb 支持格式的文件（相对路径 + 绝对路径，按相对路径排序）。
pub(crate) fn collect_drafts(shared: &Shared) -> Vec<(String, PathBuf)> {
    let project_borrow = shared.project.borrow();
    let Some(project) = project_borrow.as_ref() else {
        return Vec::new();
    };
    let root = project.root.clone();
    let Ok((store, rt)) = shared.scratchpad_store() else {
        return Vec::new();
    };
    // 深度 = 递归枚举草稿箱（对齐草稿树展开语义）
    let Ok(entries) = rt.block_on(store.list_local_entries(u32::MAX)) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    collect_flattened(&entries, &root, &mut out);
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// 递归把草稿树拍平成「相对路径 → 绝对路径」（仅保留 duckdb 支持格式）。
fn collect_flattened(
    entries: &[ScratchpadEntry],
    root: &PathBuf,
    out: &mut Vec<(String, PathBuf)>,
) {
    for entry in entries {
        match entry.kind {
            scratchpad::ScratchpadEntryKind::File => {
                let rel = entry.path.to_string_lossy().to_string();
                let abs = root.join(&entry.path);
                if engine::file_reader_function(abs.to_string_lossy().as_ref()).is_some() {
                    out.push((rel, abs));
                }
            }
            scratchpad::ScratchpadEntryKind::Folder => {
                if let Some(children) = &entry.children {
                    collect_flattened(children, root, out);
                }
            }
        }
    }
}

/// 「资产库 · 本项目」的内部表清单（`{项目}/.RSmeta/analytics.duckdb`）。
///
/// **无项目时返回空**，不再退回全局分析库：全局那份由独立的「资产库 · 全局」段
/// （`SourceKind::AnalysisGlobal`）负责。退回会让同一张全局表在两个段里各出现一次，
/// 用户无法判断自己选的是哪一份（两段并列的前提就是**不混排**）。
///
/// 查法与 `mock_generator::analysis_tables` 一致（`information_schema` + 过滤 `main` schema），
/// 但**连接必须只读**（`ProjectDatabaseManager::open_read_only` 同款策略）：
/// - 分析库是 DuckDB 单写者模型，写连接该由 `ProjectDatabaseManager` 独持；
///   只读连接不拿写锁，可与项目主写连接并发——picker 只是**列名字**，没有写理由；
/// - 反过来说，用可写模式打开一个**尚不存在**的分析库会把它**创建出来**（DuckDB 建库语义）：
///   打开一次来源选择器就在项目里平白多一个空的 `analytics.duckdb`，那是副作用，不是查询。
pub(super) fn collect_analysis_tables(project: Option<&project::ui::OpenProject>) -> Vec<String> {
    let Some(root) = project.map(|p| p.root.clone()) else {
        return Vec::new();
    };
    analysis_tables_of(&engine::persistence::project_db::ProjectDatabaseManager::analysis_db_path(&root))
}

/// 从一台分析库（DuckDB 文件）里读 `main` 与归档 `rds_archive` 的表名（只读，失败一律空）。
///
/// 归档后的表资产只存在于 `rds_archive`；漏掉这一段会让每个表型资产都被误判失效。
pub(crate) fn analysis_tables_of(path: &std::path::Path) -> Vec<String> {
    let Ok(config) = duckdb::Config::default().access_mode(duckdb::AccessMode::ReadOnly) else {
        return Vec::new();
    };
    let Ok(conn) = duckdb::Connection::open_with_flags(path, config) else {
        return Vec::new();
    };
    let mut stmt = match conn.prepare(
        "SELECT table_name FROM information_schema.tables \
         WHERE table_schema IN ('main', 'rds_archive') ORDER BY 1",
    ) {
        Ok(stmt) => stmt,
        Err(_) => return Vec::new(),
    };
    let Ok(rows) = stmt.query_map([], |row| row.get::<_, String>(0)) else {
        return Vec::new();
    };
    rows.filter_map(Result::ok).collect()
}

/// 展开某一层时的按需 introspect。
///
/// 深度规则在这里落地（与 `db.rs` 模块文档一致）：
/// - 连接：先问 catalog；只有一个就**跳过 catalog 行**，继续问它的 schema（无 schema 层则直接问表）；
/// - catalog（多库才会出现）：把各 schema 的表汇总成第 3 层，schema 折进标签；
/// - schema：只问它自己的表。
///
/// 缓存命中（含「空结果」）直接返回，所以同一层只会 introspect 一次。
pub(super) fn load_db_layer(
    state: &Rc<RefCell<PickerState>>,
    shared: &Shared,
    id: &NodeId,
    window: &mut Window,
) {
    if state.borrow().db_cache.is_loaded(id) {
        return;
    }
    let project_root = shared.project.borrow().as_ref().map(|p| p.root.clone());

    let rows = match id {
        NodeId::Conn(conn) => {
            let catalogs = query_catalogs(conn, project_root.clone());
            match catalog_level(conn, &catalogs, 2) {
                CatalogLevel::None => Vec::new(),
                CatalogLevel::Many(rows) => rows,
                CatalogLevel::Single(catalog) => {
                    let schemas = query_schemas(conn, &catalog, project_root.clone());
                    if schemas.is_empty() {
                        // 驱动无独立 schema 层（MySQL 口径）：表直接挂第 2 层
                        let tables = query_tables(conn, &catalog, "", project_root);
                        table_rows(conn, &catalog, "", &tables, 2)
                    } else {
                        schema_rows(conn, &catalog, &schemas, 2)
                    }
                }
            }
        }
        NodeId::DbCatalog { conn, catalog } => {
            let schemas = query_schemas(conn, catalog, project_root.clone());
            let tables = if schemas.is_empty() {
                query_tables(conn, catalog, "", project_root)
            } else {
                let mut all = Vec::new();
                for schema in &schemas {
                    all.extend(query_tables(conn, catalog, schema, project_root.clone()));
                }
                all
            };
            // 第 3 层放表，schema 折进标签（`schema · 表`）
            table_rows(conn, catalog, "", &tables, 3)
        }
        NodeId::DbSchema {
            conn,
            catalog,
            schema,
        } => {
            let tables = query_tables(conn, catalog, schema, project_root);
            table_rows(conn, catalog, schema, &tables, 3)
        }
        _ => return,
    };

    state.borrow_mut().db_cache.put(id.clone(), rows);
    window.refresh();
}

#[cfg(test)]
mod tests {
    // 刻意不用 `use super::*`：会把 gpui 的 `test` 属性宏带进作用域。
    use super::analysis_tables_of;

    /// 分析库里的表名要读得到；库不存在时**空**而不是「顺手建一个」——
    /// 只读打开对不存在的文件是 Err，这条同时钉住「picker 不产生副作用」。
    #[test]
    fn analysis_tables_are_read_only_and_never_create_the_db() {
        let dir = std::env::temp_dir().join(format!("rds-picker-an-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("建临时目录");
        let path = dir.join("analytics.duckdb");
        let _ = std::fs::remove_file(&path);
        let conn = duckdb::Connection::open(&path).expect("造分析库");
        conn.execute_batch("create table res_结果集_探查(id integer);")
            .expect("写持久表");
        drop(conn);

        assert_eq!(
            analysis_tables_of(&path),
            vec!["res_结果集_探查".to_string()]
        );

        let missing = dir.join("nope.duckdb");
        assert!(analysis_tables_of(&missing).is_empty(), "不存在的库不给表");
        assert!(!missing.exists(), "只读打开不该把库创建出来");

        let _ = std::fs::remove_dir_all(&dir);
    }
}

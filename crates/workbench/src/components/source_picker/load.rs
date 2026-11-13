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
    let mut out = Vec::new();
    // 有 schema 层：先展开 Schema 节点拿类别文件夹；无则 catalog 直接承载。
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
    let Ok(_) = rt.block_on(async { svc.load_children(conn_id, &path, None, false).await }) else {
        return out;
    };
    // 类别文件夹展开取表 / 视图节点（folder 路径的 schema 与展开路径一致）
    for folder in [NavFolder::Tables, NavFolder::Views] {
        let folder_path = NavPath::Folder {
            catalog: catalog.to_string(),
            schema: schema.to_string(),
            folder,
        };
        let children =
            rt.block_on(async { svc.load_children(conn_id, &folder_path, None, false).await });
        let Ok(children) = children else { continue };
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
    out
}

/// 草稿箱里 duckdb 支持格式的文件（相对路径 + 绝对路径，按相对路径排序）。
pub(super) fn collect_drafts(shared: &Shared) -> Vec<(String, PathBuf)> {
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

/// 资产库内部表清单（`{项目}/.RSmeta/analytics.duckdb`，无项目读全局分析库）。
///
/// 查法与 `mock_generator::analysis_tables` 一致（`duckdb::Connection::open` + 过滤 `main` schema）。
pub(super) fn collect_analysis_tables(project: Option<&project::ui::OpenProject>) -> Vec<String> {
    let path = project
        .map(|p| p.root.clone())
        .map(|root| {
            engine::persistence::project_db::ProjectDatabaseManager::analysis_db_path(&root)
        })
        .or_else(|| Some(connection_dialog::services::workspace_loader::global_analysis_db_path()));
    let Some(path) = path else {
        return Vec::new();
    };
    let Ok(conn) = duckdb::Connection::open(&path) else {
        return Vec::new();
    };
    let mut stmt = match conn.prepare(
        "SELECT table_name FROM information_schema.tables WHERE table_schema = 'main' ORDER BY 1",
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

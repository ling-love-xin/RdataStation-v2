//! rds-database — 导航编排服务（M4）。
//!
//! 负责把 engine 的实时内省（`MetadataService`）映射为视图友好的 `NavNode` 树，
//! 按展开路径懒加载子节点。
//!
//! Phase A 先打通「实时内省 + 懒加载」；L2 每连接缓存与预热编排在 Phase C 接入
//! （见 `docs/architecture/database/database-nav-dev-plan.md`）。
//!
//! 层次：Connection → Catalog → Schema → 类别文件夹 → 表/视图 → 列。

use std::sync::Arc;

use engine::connection_manager::ConnectionManager;
use engine::driver::traits::{SchemaObject, SchemaObjectKind};
use shared::error::CoreError;

use crate::metadata_service::MetadataService;
use crate::model::{NavFolder, NavNode, NavNodeKind, NavPath};

/// 无 Catalog 层级时的退化容器名（SQLite / DuckDB 等）。
const FALLBACK_CONTAINER: &str = "main";

/// 导航服务：按路径懒加载对象树。
pub struct NavigatorService {
    metadata: MetadataService,
}

impl NavigatorService {
    /// 用共享的连接管理器构造。
    pub fn new(manager: Arc<ConnectionManager>) -> Self {
        Self {
            metadata: MetadataService::new(manager),
        }
    }

    /// 按展开路径加载子节点。
    pub async fn load_children(
        &self,
        conn_id: &str,
        path: &NavPath,
    ) -> Result<Vec<NavNode>, CoreError> {
        match path {
            NavPath::Connection => self.load_catalogs(conn_id).await,
            NavPath::Catalog { catalog } => self.load_schemas(conn_id, catalog).await,
            NavPath::Schema { catalog, schema } => {
                self.load_folders(conn_id, catalog, schema).await
            }
            NavPath::Folder {
                catalog,
                schema,
                folder,
            } => self.load_objects(conn_id, catalog, schema, *folder).await,
            NavPath::Table {
                catalog,
                schema,
                table,
            } => self.load_columns(conn_id, catalog, schema, table).await,
        }
    }

    /// 连接根 → Catalog 列表；无 Catalog 时退化为单一 `main` 容器。
    async fn load_catalogs(&self, conn_id: &str) -> Result<Vec<NavNode>, CoreError> {
        let catalogs = self.metadata.list_catalogs(conn_id).await?;
        let names = if catalogs.is_empty() {
            vec![FALLBACK_CONTAINER.to_string()]
        } else {
            catalogs
        };
        Ok(names
            .into_iter()
            .map(|name| {
                let path = NavPath::Catalog {
                    catalog: name.clone(),
                };
                NavNode::new(
                    NavNode::child_key(conn_id, &[name.as_str()]),
                    name,
                    conn_id,
                    NavNodeKind::Catalog,
                    true,
                )
                .with_expand_path(path)
            })
            .collect())
    }

    /// Catalog → Schema 列表；无 Schema 时退化为单一 `main`。
    async fn load_schemas(&self, conn_id: &str, catalog: &str) -> Result<Vec<NavNode>, CoreError> {
        // 部分驱动（SQLite/DuckDB）不区分 catalog/schema，查询可能返回空；
        // 空结果按单一 `main` schema 处理，保证树仍可下钻。
        let schemas = self
            .metadata
            .list_schemas(conn_id, catalog)
            .await
            .unwrap_or_default();
        let names = if schemas.is_empty() {
            vec![FALLBACK_CONTAINER.to_string()]
        } else {
            schemas
        };
        Ok(names
            .into_iter()
            .map(|name| {
                let path = NavPath::Schema {
                    catalog: catalog.to_string(),
                    schema: name.clone(),
                };
                NavNode::new(
                    NavNode::child_key(conn_id, &[catalog, name.as_str()]),
                    name,
                    conn_id,
                    NavNodeKind::Schema,
                    true,
                )
                .with_expand_path(path)
            })
            .collect())
    }

    /// Schema → 类别文件夹（有对象的才显示，标题带计数）。
    async fn load_folders(
        &self,
        conn_id: &str,
        catalog: &str,
        schema: &str,
    ) -> Result<Vec<NavNode>, CoreError> {
        let mut nodes = Vec::new();
        for folder in NavFolder::ALL {
            let objects = self
                .collect_objects(conn_id, catalog, schema, folder)
                .await?;
            if objects.is_empty() {
                continue;
            }
            let count = objects.len();
            nodes.push(
                NavNode::new(
                    NavNode::child_key(conn_id, &[catalog, schema, folder.key()]),
                    format!("{} ({})", folder.label(), count),
                    conn_id,
                    NavNodeKind::Folder(folder),
                    true,
                )
                .with_expand_path(NavPath::Folder {
                    catalog: catalog.to_string(),
                    schema: schema.to_string(),
                    folder,
                }),
            );
        }
        Ok(nodes)
    }

    /// 类别文件夹 → 具体对象。
    async fn load_objects(
        &self,
        conn_id: &str,
        catalog: &str,
        schema: &str,
        folder: NavFolder,
    ) -> Result<Vec<NavNode>, CoreError> {
        let objects = self
            .collect_objects(conn_id, catalog, schema, folder)
            .await?;
        Ok(objects
            .into_iter()
            .map(|obj| {
                let (kind, has_children) = match folder {
                    NavFolder::Tables => (NavNodeKind::Table { row_estimate: None }, true),
                    NavFolder::Views => (NavNodeKind::View, true),
                    NavFolder::Routines => (
                        NavNodeKind::Routine {
                            routine_type: format!("{:?}", obj.kind),
                        },
                        false,
                    ),
                    NavFolder::Sequences => (NavNodeKind::Sequence, false),
                    NavFolder::Triggers => (NavNodeKind::Trigger, false),
                };
                let name = obj.name;
                let key = NavNode::child_key(conn_id, &[catalog, schema, name.as_str()]);
                let node = NavNode::new(key, name.clone(), conn_id, kind, has_children)
                    .with_comment(obj.comment);
                if has_children {
                    node.with_expand_path(NavPath::Table {
                        catalog: catalog.to_string(),
                        schema: schema.to_string(),
                        table: name,
                    })
                } else {
                    node
                }
            })
            .collect())
    }

    /// 表 / 视图 → 列。
    async fn load_columns(
        &self,
        conn_id: &str,
        catalog: &str,
        schema: &str,
        table: &str,
    ) -> Result<Vec<NavNode>, CoreError> {
        let columns = self
            .metadata
            .list_columns(conn_id, catalog, schema, table)
            .await?;
        Ok(columns
            .into_iter()
            .map(|col| {
                let key = NavNode::child_key(conn_id, &[catalog, schema, table, col.name.as_str()]);
                NavNode::new(
                    key,
                    col.name,
                    conn_id,
                    NavNodeKind::Column {
                        data_type: col.data_type,
                        nullable: col.nullable,
                        primary: col.is_primary_key,
                        foreign: col.is_foreign_key,
                    },
                    false,
                )
                .with_comment(col.comment)
            })
            .collect())
    }

    /// 按类别收集对象（复用 `MetadataService` 的内省调用）。
    async fn collect_objects(
        &self,
        conn_id: &str,
        catalog: &str,
        schema: &str,
        folder: NavFolder,
    ) -> Result<Vec<SchemaObject>, CoreError> {
        match folder {
            NavFolder::Tables | NavFolder::Views => {
                let objects = self.metadata.list_tables(conn_id, catalog, schema).await?;
                let want_view = folder == NavFolder::Views;
                Ok(objects
                    .into_iter()
                    .filter(|o| (o.kind == SchemaObjectKind::View) == want_view)
                    .collect())
            }
            NavFolder::Routines => {
                let mut procedures = self
                    .metadata
                    .list_procedures(conn_id, catalog, schema)
                    .await?;
                let functions = self
                    .metadata
                    .list_functions(conn_id, catalog, schema)
                    .await?;
                procedures.extend(functions);
                Ok(procedures)
            }
            NavFolder::Sequences => self.metadata.list_sequences(conn_id, catalog, schema).await,
            NavFolder::Triggers => self.metadata.list_triggers(conn_id, catalog, schema).await,
        }
    }
}

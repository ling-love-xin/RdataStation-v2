//! rds-database — 导航编排服务（M4）。
//!
//! 负责把 engine 的实时内省（`MetadataService`）映射为视图友好的 `NavNode` 树，
//! 按展开路径懒加载子节点。
//!
//! Phase A 先打通「实时内省 + 懒加载」；Phase C（C3）接入连接级 L2 缓存：
//! 读优先命中缓存（`cache::NavCache`），未命中回落实时内省并回写；`fresh` 模式跳过
//! 缓存并在重写前清理旧行（右键「刷新元数据」用）。
//!
//! 层次：Connection → Catalog → Schema → 类别文件夹 → 表/视图 → 列。

use std::sync::Arc;

use engine::connection_manager::ConnectionManager;
use engine::driver::traits::{ColumnDetail, SchemaObject, SchemaObjectKind};
use shared::error::CoreError;

use crate::cache::NavCache;
use crate::metadata_service::MetadataService;
use crate::model::{
    NavFolder, NavNode, NavNodeKind, NavPath, NavSource, PropertyKind, PropertyRef,
};

/// 无 Catalog 层级时的退化容器名（SQLite / DuckDB 等）。
const FALLBACK_CONTAINER: &str = "main";

/// 导航服务：按路径懒加载对象树（cache-aside）。
pub struct NavigatorService {
    metadata: MetadataService,
    /// 项目根（项目 / 共享连接的缓存与内省路径需要；全局连接为 None）。
    project_root: Option<String>,
    /// 刷新模式：跳过读缓存，并用实时结果重写缓存。
    fresh: bool,
}

impl NavigatorService {
    /// 用共享的连接管理器构造（无项目根、非刷新；仅全局连接的缓存在此可用）。
    pub fn new(manager: Arc<ConnectionManager>) -> Self {
        Self {
            metadata: MetadataService::new(manager),
            project_root: None,
            fresh: false,
        }
    }

    /// 带上下文构造：项目根用于定位项目侧缓存；`fresh` 为刷新模式。
    pub fn with_context(
        manager: Arc<ConnectionManager>,
        project_root: Option<String>,
        fresh: bool,
    ) -> Self {
        Self {
            metadata: MetadataService::new(manager),
            project_root,
            fresh,
        }
    }

    /// 打开该连接的 L2 缓存（不可用时返回 `None`，降级实时内省）。
    fn cache(&self, conn_id: &str) -> Option<NavCache> {
        NavCache::open(conn_id, self.project_root.as_deref())
    }

    /// 按展开路径加载子节点。
    pub async fn load_children(
        &self,
        conn_id: &str,
        path: &NavPath,
    ) -> Result<Vec<NavNode>, CoreError> {
        match path {
            NavPath::Connection => self.load_catalogs(conn_id).await,
            NavPath::Catalog { catalog } => {
                // 层级按数据库类型动态决定：无独立 Schema 层的驱动（MySQL / SQLite /
                // DuckDB）让 Catalog 直接承载类别文件夹，避免出现同名重复的 Schema 层。
                if self.metadata.has_schema_level(conn_id).await? {
                    self.load_schemas(conn_id, catalog).await
                } else {
                    self.load_folders(conn_id, catalog, catalog).await
                }
            }
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
                let prop = PropertyRef {
                    conn_id: conn_id.to_string(),
                    source: NavSource::from_conn_id(conn_id),
                    catalog: Some(name.clone()),
                    schema: None,
                    parent: None,
                    name: name.clone(),
                    kind: PropertyKind::Catalog,
                };
                NavNode::new(
                    NavNode::child_key(conn_id, &[name.as_str()]),
                    name,
                    conn_id,
                    NavNodeKind::Catalog,
                    true,
                )
                .with_expand_path(path)
                .with_property(prop)
            })
            .collect())
    }

    /// Catalog → Schema 列表；无 Schema 时退化为单一 `main`。
    async fn load_schemas(&self, conn_id: &str, catalog: &str) -> Result<Vec<NavNode>, CoreError> {
        // 部分驱动（SQLite/DuckDB）不区分 catalog/schema，查询可能返回空；
        // 空结果按单一 `main` schema 处理，保证树仍可下钻。
        let names = self.schema_names(conn_id, catalog).await?;
        Ok(names
            .into_iter()
            .map(|name| {
                let path = NavPath::Schema {
                    catalog: catalog.to_string(),
                    schema: name.clone(),
                };
                let prop = PropertyRef {
                    conn_id: conn_id.to_string(),
                    source: NavSource::from_conn_id(conn_id),
                    catalog: Some(catalog.to_string()),
                    schema: Some(name.clone()),
                    parent: None,
                    name: name.clone(),
                    kind: PropertyKind::Schema,
                };
                NavNode::new(
                    NavNode::child_key(conn_id, &[catalog, name.as_str()]),
                    name,
                    conn_id,
                    NavNodeKind::Schema,
                    true,
                )
                .with_expand_path(path)
                .with_property(prop)
            })
            .collect())
    }

    /// schema 名称（cache-aside：非刷新时命中缓存直接返回；未命中内省并回写）。
    async fn schema_names(&self, conn_id: &str, catalog: &str) -> Result<Vec<String>, CoreError> {
        let cache = self.cache(conn_id);
        if !self.fresh {
            if let Some(c) = &cache {
                if let Some(names) = c.schemas(catalog) {
                    return Ok(names);
                }
            }
        }
        let live = self.metadata.list_schemas(conn_id, catalog).await?;
        let names = if live.is_empty() {
            vec![FALLBACK_CONTAINER.to_string()]
        } else {
            live
        };
        if let Some(c) = &cache {
            c.put_schemas(catalog, &names);
        }
        Ok(names)
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
                // 所有类别对象都带属性定位信息：此前仅表 / 视图带，导致例程 / 序列 /
                // 触发器节点既无「查看属性」也无「查看源码」。
                let kind_prop = PropertyRef {
                    conn_id: conn_id.to_string(),
                    source: NavSource::from_conn_id(conn_id),
                    catalog: Some(catalog.to_string()),
                    schema: Some(schema.to_string()),
                    parent: None,
                    name: name.clone(),
                    kind: match folder {
                        NavFolder::Tables => PropertyKind::Table,
                        NavFolder::Views => PropertyKind::View,
                        NavFolder::Routines => PropertyKind::Routine,
                        NavFolder::Sequences => PropertyKind::Sequence,
                        NavFolder::Triggers => PropertyKind::Trigger,
                    },
                };
                let node = NavNode::new(key, name.clone(), conn_id, kind, has_children)
                    .with_comment(obj.comment)
                    .with_property(kind_prop);
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

    /// 表 / 视图 → 列（cache-aside）。
    async fn load_columns(
        &self,
        conn_id: &str,
        catalog: &str,
        schema: &str,
        table: &str,
    ) -> Result<Vec<NavNode>, CoreError> {
        let cache = self.cache(conn_id);
        let schema_id = cache.as_ref().and_then(|c| c.schema_id(catalog, schema));
        if !self.fresh {
            if let (Some(c), Some(sid)) = (&cache, schema_id) {
                if let Some(cols) = c.columns(sid, table) {
                    return Ok(Self::column_nodes(conn_id, catalog, schema, table, cols));
                }
            }
        }
        let columns = self
            .metadata
            .list_columns(conn_id, catalog, schema, table)
            .await?;
        if let (Some(c), Some(sid)) = (&cache, schema_id) {
            c.put_columns(sid, table, &columns);
        }
        Ok(Self::column_nodes(conn_id, catalog, schema, table, columns))
    }

    /// 列详情 → 导航节点（与缓存读写解耦）。
    fn column_nodes(
        conn_id: &str,
        catalog: &str,
        schema: &str,
        table: &str,
        columns: Vec<ColumnDetail>,
    ) -> Vec<NavNode> {
        columns
            .into_iter()
            .map(|col| {
                let key = NavNode::child_key(conn_id, &[catalog, schema, table, col.name.as_str()]);
                let prop = PropertyRef {
                    conn_id: conn_id.to_string(),
                    source: NavSource::from_conn_id(conn_id),
                    catalog: Some(catalog.to_string()),
                    schema: Some(schema.to_string()),
                    parent: Some(table.to_string()),
                    name: col.name.clone(),
                    kind: PropertyKind::Column,
                };
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
                .with_property(prop)
            })
            .collect()
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
                let want_view = folder == NavFolder::Views;
                let mut cache = self.cache(conn_id);
                let mut schema_id = cache.as_ref().and_then(|c| c.schema_id(catalog, schema));
                if !self.fresh {
                    if let (Some(c), Some(sid)) = (&cache, schema_id) {
                        if let Some(rows) = c.objects(sid, want_view) {
                            return Ok(rows
                                .into_iter()
                                .map(|(name, comment)| SchemaObject {
                                    name,
                                    kind: if want_view {
                                        SchemaObjectKind::View
                                    } else {
                                        SchemaObjectKind::Table
                                    },
                                    children: None,
                                    comment,
                                    table_name: None,
                                    event: None,
                                })
                                .collect());
                        }
                    }
                }
                let objects = self.metadata.list_tables(conn_id, catalog, schema).await?;
                let filtered: Vec<SchemaObject> = objects
                    .into_iter()
                    .filter(|o| (o.kind == SchemaObjectKind::View) == want_view)
                    .collect();
                if let Some(c) = cache.as_mut() {
                    if self.fresh {
                        // 刷新：先清该 schema 旧行再整块重写，避免已删对象残留在缓存。
                        c.prune_schema(catalog, schema);
                        let name = schema.to_string();
                        c.put_schemas(catalog, std::slice::from_ref(&name));
                        schema_id = c.schema_id(catalog, schema);
                    }
                    if let Some(sid) = schema_id {
                        c.put_objects(sid, &filtered);
                    }
                }
                Ok(filtered)
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

    /// C1 预热（方案 C）：内省 catalogs / schemas 并写入 L2。
    ///
    /// `on_total` 在拿到 catalog 总数后回调一次；`on_step` 每处理完一个 catalog 回调；
    /// `is_cancelled` 在每个 catalog 之间检查（取消不报错，返回已处理数）。
    pub async fn warm_schemas(
        &self,
        conn_id: &str,
        on_total: impl Fn(usize),
        is_cancelled: impl Fn() -> bool,
        on_step: impl Fn(usize),
    ) -> Result<usize, CoreError> {
        let catalogs = self
            .metadata
            .list_catalogs(conn_id)
            .await
            .unwrap_or_default();
        let catalogs = if catalogs.is_empty() {
            vec![FALLBACK_CONTAINER.to_string()]
        } else {
            catalogs
        };
        on_total(catalogs.len());
        let mut done = 0usize;
        for catalog in catalogs {
            if is_cancelled() {
                break;
            }
            if let Ok(names) = self.metadata.list_schemas(conn_id, &catalog).await {
                let names = if names.is_empty() {
                    vec![FALLBACK_CONTAINER.to_string()]
                } else {
                    names
                };
                // 缓存句柄作用域内无 await（保持 future 对后台执行器友好）。
                if let Some(cache) = self.cache(conn_id) {
                    cache.put_schemas(&catalog, &names);
                }
            }
            done += 1;
            on_step(done);
        }
        Ok(done)
    }

    /// C2 邻接预取：预取指定表 / 视图的列（命中 L2 则不再内省；失败静默）。
    pub async fn prefetch_columns(
        &self,
        conn_id: &str,
        targets: &[(String, String, String)],
    ) -> usize {
        let mut ok = 0usize;
        for (catalog, schema, table) in targets {
            if self
                .load_columns(conn_id, catalog, schema, table)
                .await
                .is_ok()
            {
                ok += 1;
            }
        }
        ok
    }

    /// 加载对象属性（属性面板用；连接 / Catalog / Schema 无需查询）。
    pub async fn load_properties(
        &self,
        ref_: &PropertyRef,
        conn_label: &str,
        driver: &str,
        db_type: Option<&str>,
    ) -> Result<crate::property_panel::ObjectProperties, CoreError> {
        crate::property_panel::load_properties(&self.metadata, ref_, conn_label, driver, db_type)
            .await
    }
}

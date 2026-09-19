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
//!
//! 分页（C4）：类别文件夹支持按 `metadata_index` 分块加载（`load_children_page`）。
//! 小 schema（≤ [`CHUNK_THRESHOLD`]）保持「一次拉全 + 进程内 L1」的既有路径，
//! 大 schema 才走索引分块——两者行为差异只在阈值两侧，避免为了分页把热路径复杂化。

use std::sync::Arc;

use engine::cache::{CacheManager, MetadataCache};
use engine::connection_manager::ConnectionManager;
use engine::driver::traits::{ColumnDetail, NodeInfo, SchemaObjectKind};
use engine::persistence::SchemaObjectCounts;
use shared::error::CoreError;

use crate::cache::NavCache;
use crate::metadata_service::MetadataService;
use crate::model::{
    NavFolder, NavNode, NavNodeKind, NavPath, NavSource, PropertyKind, PropertyRef,
};

/// 无 Catalog 层级时的退化容器名（SQLite / DuckDB 等）。
const FALLBACK_CONTAINER: &str = "main";

/// 走索引分块而非一次拉全的对象数阈值（单类别，表 / 视图各自判断）。
///
/// 为什么取 500：UI 首批渲染 200 条（`workbench_shell::ui::NAV_FOLDER_PAGE_SIZE`），
/// 500 以内一次拉全的代价（命中 L1 是内存拷贝）小于多一次后台往返；超过 500 后
/// 首个屏的耗时与常驻内存都随对象数线性上长，才值得付出分页的复杂度。
/// 同样重要的原因：阈值以下完全走旧路径 = 零回归风险。
pub const CHUNK_THRESHOLD: usize = 500;

/// 一页导航子节点（分块加载的返回值）。
///
/// 为什么需要 `total`：客户端分页要知道“还有多少没拉”。仅靠已加载条数推算不出——
/// 大 schema 的已加载数永远停在首批上，于是「加载更多」要么不出现、要么无限出现。
#[derive(Debug, Clone)]
pub struct NavPage {
    /// 本页节点。
    pub nodes: Vec<NavNode>,
    /// 该路径下的对象总数（大 schema 来自 `metadata_index` 计数，小 schema 来自全量结果长度）。
    pub total: usize,
    /// 本页起始下标（0 = 首屏）。
    pub offset: usize,
    /// 是否还有后续页。
    pub has_more: bool,
}

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

    // ==================== L1（进程内内存缓存） ====================
    //
    // 三级设计口径（`docs/architecture/database/README.md:32`）：
    //   L1 进程内内存（<0.1ms） → L2 每连接 SQLite（<5ms） → L3 实时内省（10~500ms，**不是缓存**）
    //
    // 历史状态：L1 只被「连接断开时失效」调过一次，**从没有人写入** —— 因此恒空，
    // 每次读都要付 `NavCache::open` 的固定开销（开文件 + 5 条 PRAGMA + 一次迁移校验）。
    // 这里补齐读写：命中即返回；L2 命中与实时内省的结果都回填 L1。

    /// L1 句柄（进程级单例；取不到时降级为不缓存，不影响主流程）。
    fn l1() -> Option<Arc<std::sync::Mutex<MetadataCache>>> {
        CacheManager::instance().lock().ok().map(|m| m.metadata_cache())
    }

    /// 读 L1。`fresh`（刷新）模式一律跳过：刷新以实时内省为准。
    fn l1_read<T>(&self, read: impl FnOnce(&mut MetadataCache) -> Option<T>) -> Option<T> {
        if self.fresh {
            return None;
        }
        let cache = Self::l1()?;
        let mut guard = cache.lock().ok()?;
        read(&mut guard)
    }

    /// 写 L1（尽力而为；失败不影响主流程）。
    fn l1_write(&self, write: impl FnOnce(&mut MetadataCache)) {
        let Some(cache) = Self::l1() else { return };
        if let Ok(mut guard) = cache.lock() {
            write(&mut guard);
        }
    }

    /// 刷新时清掉该连接的 L1（L2 由各路径按需重写）。
    ///
    /// 设计口径（原型 §刷新规则）：「手动刷新 → 清 L1；L2 标记 stale」。
    fn l1_invalidate_connection(&self, conn_id: &str) {
        if let Ok(manager) = CacheManager::instance().lock() {
            manager.invalidate_connection(conn_id);
        }
    }

    /// 按展开路径加载子节点。
    pub async fn load_children(
        &self,
        conn_id: &str,
        path: &NavPath,
    ) -> Result<Vec<NavNode>, CoreError> {
        // 刷新语义：以实时内省为准，先清该连接的 L1（L2 由各路径重写）。
        if self.fresh {
            self.l1_invalidate_connection(conn_id);
        }

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

    /// 按展开路径加载子节点（支持分页：`offset` / `limit` 对类别文件夹生效）。
    ///
    /// 非「类别文件夹」路径只有一个隐含的整页（忽略 `offset`、不裁 `limit`）：
    /// catalog / schema / 列的数量级远小于表对象，分页收益不抵复杂度。
    pub async fn load_children_page(
        &self,
        conn_id: &str,
        path: &NavPath,
        offset: usize,
        limit: usize,
    ) -> Result<NavPage, CoreError> {
        match path {
            NavPath::Folder {
                catalog,
                schema,
                folder,
            } => {
                self.load_object_page(conn_id, catalog, schema, *folder, offset, limit)
                    .await
            }
            _ => {
                let nodes = self.load_children(conn_id, path).await?;
                let total = nodes.len();
                Ok(NavPage {
                    nodes,
                    total,
                    offset: 0,
                    has_more: false,
                })
            }
        }
    }

    /// 类别文件夹 → 一页对象。
    ///
    /// 优先走 `metadata_index` 分块（仅大 schema）；否则回落既有全量路径
    /// （L1 → L2 → 实时内省，并在写 L2 时重建索引）。
    async fn load_object_page(
        &self,
        conn_id: &str,
        catalog: &str,
        schema: &str,
        folder: NavFolder,
        offset: usize,
        limit: usize,
    ) -> Result<NavPage, CoreError> {
        if let Some(page) = self.index_page(conn_id, catalog, schema, folder, offset, limit) {
            return Ok(page);
        }

        // 全量回退：索引不可用（冷启动 / 该类别为空）或对象数未超阈值。
        // 这一趟会把实时结果写 L2 并在表文件夹这趟重建索引，下次展开即可分块。
        let objects = self
            .collect_objects(conn_id, catalog, schema, folder)
            .await?;
        let total = objects.len();
        let nodes = Self::object_nodes(conn_id, catalog, schema, folder, objects);
        Ok(NavPage {
            nodes,
            total,
            offset: 0,
            has_more: false,
        })
    }

    /// 从 `metadata_index` 取一页（表 / 视图）。
    ///
    /// 返回 `None` 的四种情形（全部由调用方回退全量路径）：
    /// 1. `fresh`（刷新）：必须看实时结果，索引是刷新前的旧数据；
    /// 2. 缓存不可用 / 没有 `schema_id`（项目连接缺项目根等）；
    /// 3. 索引尚未重建（冷启动：计数全 0）；
    /// 4. 该类别未超 [`CHUNK_THRESHOLD`]（小 schema 走 L1 命中更快）。
    fn index_page(
        &self,
        conn_id: &str,
        catalog: &str,
        schema: &str,
        folder: NavFolder,
        offset: usize,
        limit: usize,
    ) -> Option<NavPage> {
        if self.fresh || !matches!(folder, NavFolder::Tables | NavFolder::Views) {
            return None;
        }
        let want_view = folder == NavFolder::Views;
        let cache = self.cache(conn_id)?;
        let schema_id = cache.schema_id(catalog, schema)?;
        let counts = cache.object_counts(schema_id)?;
        let total = if want_view {
            counts.view_count
        } else {
            counts.table_count
        };
        if total <= CHUNK_THRESHOLD {
            return None;
        }
        let chunk = cache.objects_chunk(schema_id, want_view, offset, limit)?;

        // 索引只有名字（没有注释列），故这里只映射名称与类别；
        // 排序按索引的稳定顺序（名称升序）——分块翻页不能有随机顺序。
        let objects: Vec<NodeInfo> = chunk
            .items
            .iter()
            .map(|entry| {
                NodeInfo::new(
                    entry.object_name.clone(),
                    if want_view {
                        SchemaObjectKind::View
                    } else {
                        SchemaObjectKind::Table
                    },
                )
            })
            .collect();
        let nodes = Self::object_nodes(conn_id, catalog, schema, folder, objects);
        Some(NavPage {
            nodes,
            total: chunk.total,
            offset: chunk.offset,
            has_more: chunk.has_more,
        })
    }

    /// 目标对象在其类别文件夹里的**零基位次**（仅表 / 视图这类会分页的文件夹有意义）。
    ///
    /// 与 [`Self::index_page`] 用同一套前提：拿不到缓存 / 没 `schema_id` / 索引里没这个对象，
    /// 一律返回 `None`——调用方据此**如实告知**（而不是跳到第一页假装成功）。
    pub fn object_position(&self, conn_id: &str, path: &NavPath, name: &str) -> Option<usize> {
        let NavPath::Folder {
            catalog,
            schema,
            folder,
        } = path
        else {
            return None;
        };
        if !matches!(folder, NavFolder::Tables | NavFolder::Views) {
            return None;
        }
        let cache = self.cache(conn_id)?;
        let schema_id = cache.schema_id(catalog, schema)?;
        cache.object_position(schema_id, *folder == NavFolder::Views, name)
    }

    /// 连接根 → Catalog 列表；无 Catalog 时退化为单一 `main` 容器。
    async fn load_catalogs(&self, conn_id: &str) -> Result<Vec<NavNode>, CoreError> {
        // L1 → L3（Catalogs 不在 L2 缓存范围内）
        let catalogs = match self.l1_read(|c| c.get_catalogs(conn_id)) {
            Some(names) => names,
            None => {
                let live = self.metadata.list_catalogs(conn_id).await?;
                if !live.is_empty() {
                    let backfill = live.clone();
                    self.l1_write(move |c| c.set_catalogs(conn_id, backfill));
                }
                live
            }
        };
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
        // L1
        if let Some(names) = self.l1_read(|c| c.get_schemas(conn_id, catalog)) {
            return Ok(names);
        }

        let cache = self.cache(conn_id);
        if !self.fresh {
            if let Some(c) = &cache {
                if let Some(names) = c.schemas(catalog) {
                    // 命中 L2 → 回填 L1（这是「L1 命中 <0.1ms」的来源）
                    let backfill = names.clone();
                    self.l1_write(move |l1| l1.set_schemas(conn_id, catalog, backfill));
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
        let store = names.clone();
        self.l1_write(move |l1| l1.set_schemas(conn_id, catalog, store));
        Ok(names)
    }

    /// Schema → 类别文件夹（有对象的才显示，标题带计数）。
    ///
    /// 「表」的计数优先取 `metadata_index`（索引可用时）：这样展开 schema 这一层
    /// **不必把整个表清单物化一遍**，只为了在标题里写个数字。
    /// 其余类别仍走全量收集（见 [`Self::indexed_folder_count`] 的说明）。
    async fn load_folders(
        &self,
        conn_id: &str,
        catalog: &str,
        schema: &str,
    ) -> Result<Vec<NavNode>, CoreError> {
        let indexed = self.indexed_counts(conn_id, catalog, schema);
        let mut nodes = Vec::new();
        for folder in NavFolder::ALL {
            let count = match Self::indexed_folder_count(folder, indexed.as_ref()) {
                Some(count) => count,
                None => {
                    // 实时内省（或 L1 / L2 命中）；失败向上冒泡，不倒空文件夹。
                    self.collect_objects(conn_id, catalog, schema, folder)
                        .await?
                        .len()
                }
            };
            if count == 0 {
                continue;
            }
            nodes.push(Self::folder_node(conn_id, catalog, schema, folder, count));
        }
        Ok(nodes)
    }

    /// 某类别的计数能否直接信索引；`None` = 必须全量收集。
    ///
    /// **只有「表」能信**（索引重建挂在表文件夹那趟：一次实时内省同时拿到表与视图，
    /// 重建会覆盖两者）。而视图是**另一趟**才写进 L2 的（先展开表、以后再展开视图），
    /// 那一趟不重建索引——此时索引里 `view_count = 0`，照它判定就会把**视图文件夹整支隐藏**。
    /// 例程 / 序列 / 触发器根本不在索引里（不在缓存范围内），自然也不能信。
    fn indexed_folder_count(
        folder: NavFolder,
        counts: Option<&SchemaObjectCounts>,
    ) -> Option<usize> {
        match (folder, counts) {
            (NavFolder::Tables, Some(counts)) => Some(counts.table_count),
            _ => None,
        }
    }

    /// 索引里的对象计数（表 / 视图）；索引不可用或刷新模式返回 `None`。
    fn indexed_counts(
        &self,
        conn_id: &str,
        catalog: &str,
        schema: &str,
    ) -> Option<SchemaObjectCounts> {
        // 刷新必须看实时结果：索引（与 L2）都是刷新前的旧值。
        if self.fresh {
            return None;
        }
        let cache = self.cache(conn_id)?;
        let schema_id = cache.schema_id(catalog, schema)?;
        cache.object_counts(schema_id)
    }

    /// 类别文件夹节点（标题带计数）。
    fn folder_node(
        conn_id: &str,
        catalog: &str,
        schema: &str,
        folder: NavFolder,
        count: usize,
    ) -> NavNode {
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
        })
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
        Ok(Self::object_nodes(conn_id, catalog, schema, folder, objects))
    }

    /// 对象 → 导航节点（分页路径与全量路径共用，保证同一个对象在两路上长得一样）。
    fn object_nodes(
        conn_id: &str,
        catalog: &str,
        schema: &str,
        folder: NavFolder,
        objects: Vec<NodeInfo>,
    ) -> Vec<NavNode> {
        objects
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
                // `parent_name`（驱动内省给的所属表，目前只有触发器有）进 `PropertyRef.parent`，
                // 属性面板据此显示「关联表」——引用族里 `ObjectRef::trigger` 的 `parent` 同理。
                let kind_prop = PropertyRef {
                    conn_id: conn_id.to_string(),
                    source: NavSource::from_conn_id(conn_id),
                    catalog: Some(catalog.to_string()),
                    schema: Some(schema.to_string()),
                    parent: obj.parent_name.clone(),
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
            .collect()
    }

    /// 例程的 L1 读（L1 分「过程」「函数」两个键，这里合并为一份）。
    ///
    /// 合并只在这两个方法里发生：导航的「存储过程 / 函数」是一个文件夹，
    /// 而 L1 的键按类别分开（写侧也分开）。
    fn l1_read_routines(
        &self,
        conn_id: &str,
        catalog: &str,
        schema: &str,
    ) -> Option<Vec<NodeInfo>> {
        self.l1_read(|c| {
            let procs = c.get_procedures(conn_id, catalog, Some(schema));
            let funcs = c.get_functions(conn_id, catalog, Some(schema));
            if procs.is_none() && funcs.is_none() {
                return None;
            }
            let mut all = procs.unwrap_or_default();
            all.extend(funcs.unwrap_or_default());
            Some(all)
        })
    }

    /// 例程的 L1 写（按类别分写两个键）。
    fn l1_write_routines(&self, conn_id: &str, catalog: &str, schema: &str, objs: Vec<NodeInfo>) {
        let (funcs, procs): (Vec<NodeInfo>, Vec<NodeInfo>) = objs
            .into_iter()
            .partition(|o| o.kind == SchemaObjectKind::Function);
        self.l1_write(move |l1| {
            l1.set_procedures(conn_id, catalog, Some(schema), procs);
            l1.set_functions(conn_id, catalog, Some(schema), funcs);
        });
    }

    /// 表 / 视图 → 列（cache-aside）。
    async fn load_columns(
        &self,
        conn_id: &str,
        catalog: &str,
        schema: &str,
        table: &str,
    ) -> Result<Vec<NavNode>, CoreError> {
        // L1
        if let Some(cols) =
            self.l1_read(|c| c.get_columns_detail(conn_id, catalog, Some(schema), table))
        {
            return Ok(Self::column_nodes(conn_id, catalog, schema, table, cols));
        }

        let cache = self.cache(conn_id);
        let schema_id = cache.as_ref().and_then(|c| c.schema_id(catalog, schema));
        if !self.fresh {
            if let (Some(c), Some(sid)) = (&cache, schema_id) {
                if let Some(cols) = c.columns(sid, table) {
                    // 命中 L2 → 回填 L1
                    let backfill = cols.clone();
                    self.l1_write(move |l1| {
                        l1.set_columns_detail(conn_id, catalog, Some(schema), table, backfill)
                    });
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
        let store = columns.clone();
        self.l1_write(move |l1| {
            l1.set_columns_detail(conn_id, catalog, Some(schema), table, store)
        });
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
    ) -> Result<Vec<NodeInfo>, CoreError> {
        match folder {
            NavFolder::Tables | NavFolder::Views => {
                let want_view = folder == NavFolder::Views;

                // L1：表与视图分键（下面一次实时内省会两半都写进去）
                if let Some(objs) = self.l1_read(|c| {
                    if want_view {
                        c.get_views(conn_id, catalog, Some(schema))
                    } else {
                        c.get_tables(conn_id, catalog, Some(schema))
                    }
                }) {
                    return Ok(objs);
                }

                let mut cache = self.cache(conn_id);
                let mut schema_id = cache.as_ref().and_then(|c| c.schema_id(catalog, schema));
                if !self.fresh {
                    if let (Some(c), Some(sid)) = (&cache, schema_id) {
                        if let Some(rows) = c.objects(sid, want_view) {
                            let objs: Vec<NodeInfo> = rows
                                .into_iter()
                                .map(|(name, comment)| {
                                    NodeInfo::new(
                                        name,
                                        if want_view {
                                            SchemaObjectKind::View
                                        } else {
                                            SchemaObjectKind::Table
                                        },
                                    )
                                    .with_comment(comment)
                                })
                                .collect();
                            // 命中 L2 → 回填 L1
                            // （大 schema 不回填：与下方实时路径同一条门禁）
                            if objs.len() <= CHUNK_THRESHOLD {
                                let backfill = objs.clone();
                                self.l1_write(move |l1| {
                                    if want_view {
                                        l1.set_views(conn_id, catalog, Some(schema), backfill)
                                    } else {
                                        l1.set_tables(conn_id, catalog, Some(schema), backfill)
                                    }
                                });
                            }
                            return Ok(objs);
                        }
                    }
                }
                let objects = self.metadata.list_tables(conn_id, catalog, schema).await?;
                let object_count = objects.len();
                // 内省级别自适应（对标 DataGrip）：按**本 schema 的对象数**定级并登记到连接。
                // 目前唯一的消费者是 C2 邻接预取（级别 <3 时不为大 schema 预取列）。
                engine::driver::set_level(
                    conn_id,
                    engine::driver::IntrospectionLevel::from_object_count(object_count),
                );
                // 一次实时内省同时拿到表与视图：两半都进 L1，
                // 下次展开另一半也是进程内命中（不用再跑这条 SQL）。
                let (tables_half, views_half): (Vec<NodeInfo>, Vec<NodeInfo>) = objects
                    .into_iter()
                    .partition(|o| o.kind != SchemaObjectKind::View);
                let filtered: Vec<NodeInfo> = if want_view {
                    views_half.clone()
                } else {
                    tables_half.clone()
                };
                // **大 schema 不进 L1**：L1 是进程内常驻，万级对象的 `Vec<NodeInfo>`
                // 会一直占着内存；而这类 schema 的下一次展开会走索引分块（`index_page`），
                // 本来就不读 L1，写了也白占。
                // L2 照写不误（落到磁盘，按需分页读），只是不在内存里留整份。
                if object_count <= CHUNK_THRESHOLD {
                    self.l1_write(move |l1| {
                        l1.set_tables(conn_id, catalog, Some(schema), tables_half);
                        l1.set_views(conn_id, catalog, Some(schema), views_half);
                    });
                }
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
                    // 索引写入侧：**只在表文件夹这一趟**做。
                    // 一次实时内省同时拿到表与视图，重建会覆盖两者；
                    // Views 那趟再重建一次就是白干（整表删+插）。
                    if folder == NavFolder::Tables {
                        c.rebuild_index(catalog, schema);
                    }
                }
                Ok(filtered)
            }
            NavFolder::Routines => {
                // 与表 / 视图同一套 cache-aside：L1 → L2 → L3（回填两处）
                if let Some(objs) = self.l1_read_routines(conn_id, catalog, schema) {
                    return Ok(objs);
                }
                let cache = self.cache(conn_id);
                let schema_id = cache.as_ref().and_then(|c| c.schema_id(catalog, schema));
                if !self.fresh {
                    if let (Some(c), Some(sid)) = (&cache, schema_id) {
                        if let Some(objs) = c.routines(sid) {
                            self.l1_write_routines(conn_id, catalog, schema, objs.clone());
                            return Ok(objs);
                        }
                    }
                }
                let mut procedures = self
                    .metadata
                    .list_procedures(conn_id, catalog, schema)
                    .await?;
                let functions = self
                    .metadata
                    .list_functions(conn_id, catalog, schema)
                    .await?;
                procedures.extend(functions);
                if let (Some(c), Some(sid)) = (cache.as_ref(), schema_id) {
                    c.put_routines(sid, &procedures);
                }
                self.l1_write_routines(conn_id, catalog, schema, procedures.clone());
                Ok(procedures)
            }
            NavFolder::Sequences => {
                if let Some(objs) = self.l1_read(|c| c.get_sequences(conn_id, catalog, Some(schema))) {
                    return Ok(objs);
                }
                let cache = self.cache(conn_id);
                let schema_id = cache.as_ref().and_then(|c| c.schema_id(catalog, schema));
                if !self.fresh {
                    if let (Some(c), Some(sid)) = (&cache, schema_id) {
                        if let Some(objs) = c.sequences(sid) {
                            let backfill = objs.clone();
                            self.l1_write(move |l1| {
                                l1.set_sequences(conn_id, catalog, Some(schema), backfill)
                            });
                            return Ok(objs);
                        }
                    }
                }
                let objs = self.metadata.list_sequences(conn_id, catalog, schema).await?;
                if let (Some(c), Some(sid)) = (cache.as_ref(), schema_id) {
                    c.put_sequences(sid, &objs);
                }
                let store = objs.clone();
                self.l1_write(move |l1| l1.set_sequences(conn_id, catalog, Some(schema), store));
                Ok(objs)
            }
            NavFolder::Triggers => {
                if let Some(objs) = self.l1_read(|c| c.get_triggers(conn_id, catalog, Some(schema))) {
                    return Ok(objs);
                }
                let cache = self.cache(conn_id);
                let schema_id = cache.as_ref().and_then(|c| c.schema_id(catalog, schema));
                if !self.fresh {
                    if let (Some(c), Some(sid)) = (&cache, schema_id) {
                        if let Some(objs) = c.triggers(sid) {
                            let backfill = objs.clone();
                            self.l1_write(move |l1| {
                                l1.set_triggers(conn_id, catalog, Some(schema), backfill)
                            });
                            return Ok(objs);
                        }
                    }
                }
                let objs = self.metadata.list_triggers(conn_id, catalog, schema).await?;
                if let (Some(c), Some(sid)) = (cache.as_ref(), schema_id) {
                    c.put_triggers(sid, &objs);
                }
                let store = objs.clone();
                self.l1_write(move |l1| l1.set_triggers(conn_id, catalog, Some(schema), store));
                Ok(objs)
            }
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
        // 大 schema 不预取列：级别降为 Level1（对象数 > 3000，见 `from_object_count`）时，
        // “预先拉一遍所有列”的体验收益抵不过给源库加的负载与本地缓存膨胀；
        // 列仍会在用户真正展开表时按需加载（L2/L1 缓存照常生效）。
        if !engine::driver::get_level(conn_id).should_load_columns() {
            return 0;
        }

        let mut ok = 0usize;
        for (catalog, schema, table) in targets {
            // 预取失败不影响可见状态（用户真正展开那张表时会再试一次），所以这里不计入错误；
            // 但「展开表后列迟迟不出来」的第一处证据正在这里，留 DEBUG 痕。
            match self.load_columns(conn_id, catalog, schema, table).await {
                Ok(_) => ok += 1,
                Err(e) => tracing::debug!(
                    conn_id = %conn_id,
                    catalog = %catalog,
                    schema = %schema,
                    table = %table,
                    error = %e,
                    "列预取失败（不影响可见状态；日志级别调 DEBUG 可查）"
                ),
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

#[cfg(test)]
mod object_node_tests {
    //! 对象 → 导航节点的字段搬运（纯函数，不涉及缓存与连接）。

    use super::*;

    /// 触发器节点的属性定位带上驱动内省给的所属表（`NodeInfo::parent_name` → `PropertyRef.parent`）。
    ///
    /// 这条链路此前是断的：postgres 的 `list_triggers` 查了 `event_object_table`、
    /// 也填进了旧类型 `SchemaObject.table_name`，但上层两处转换都把它丢掉，
    /// 属性面板只能显示「名称 / 限定名 / 归属域」。现在它一路走到 `PropertyRef.parent`。
    #[test]
    fn trigger_nodes_carry_their_table_into_the_property_ref() {
        let nodes = NavigatorService::object_nodes(
            "P_1",
            "main",
            "public",
            NavFolder::Triggers,
            vec![NodeInfo::new("audit_trg", SchemaObjectKind::Trigger).with_parent("orders")],
        );

        assert_eq!(nodes.len(), 1);
        assert_eq!(
            nodes[0].property.as_ref().and_then(|p| p.parent.as_deref()),
            Some("orders"),
            "所属表要进属性定位，属性面板据此显示「关联表」"
        );
    }

    /// 表 / 视图没有父对象：`parent` 必须保持 `None`（而不是被填成空串）。
    #[test]
    fn table_nodes_have_no_parent() {
        let nodes = NavigatorService::object_nodes(
            "P_1",
            "main",
            "public",
            NavFolder::Tables,
            vec![NodeInfo::new("t1", SchemaObjectKind::Table)],
        );
        assert_eq!(
            nodes[0].property.as_ref().and_then(|p| p.parent.clone()),
            None
        );
    }
}

#[cfg(test)]
mod l1_tests {
    //! L1（进程内元数据缓存）接线测试。
    //!
    //! 手法：给一个**空的**连接管理器 —— 任何走实时内省（L3）的路径都必然失败
    //! （`CONN_NOT_FOUND`）。因此「L1 命中路径返回 Ok」本身就证明它没有往下走。
    //!
    //! 背景：L1 历史上只被「连接断开时失效」调用过、从没有写入方，恒空；
    //! 本次补齐写入后，这些测试锁住三件事：命中、回填、刷新时跳过。

    use std::sync::Arc;

    use engine::cache::{CacheManager, MetadataCache};
    use engine::connection_manager::ConnectionManager;

    use super::*;

    fn l1() -> Arc<std::sync::Mutex<MetadataCache>> {
        CacheManager::instance()
            .lock()
            .expect("CacheManager 单例")
            .metadata_cache()
    }

    /// 空连接管理器：任何实时内省都会失败。
    fn offline_service(fresh: bool) -> NavigatorService {
        NavigatorService::with_context(Arc::new(ConnectionManager::new()), None, fresh)
    }

    fn table(name: &str) -> NodeInfo {
        NodeInfo::new(name, SchemaObjectKind::Table)
    }

    fn column(name: &str) -> ColumnDetail {
        ColumnDetail {
            name: name.to_string(),
            data_type: "INTEGER".to_string(),
            nullable: false,
            is_primary_key: true,
            is_foreign_key: false,
            default_value: None,
            comment: None,
            extra: Default::default(),
        }
    }

    #[tokio::test]
    async fn catalogs_are_served_from_l1() {
        let conn_id = "P_l1_test_catalogs";
        {
            let cache = l1();
            let mut guard = cache.lock().expect("锁 L1");
            guard.invalidate_connection(conn_id);
            guard.set_catalogs(conn_id, vec!["main".to_string()]);
        }

        let svc = offline_service(false);
        let nodes = svc
            .load_children(conn_id, &NavPath::Connection)
            .await
            .expect("L1 命中应成功（无需真实连接）");
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].name, "main");

        // 反证：同一服务下，没有 L1 条目的连接必须失败（否则上面的断言无意义）
        assert!(svc
            .load_children("P_l1_test_no_such_conn", &NavPath::Connection)
            .await
            .is_err());
    }

    #[tokio::test]
    async fn tables_and_columns_are_served_from_l1() {
        let conn_id = "P_l1_test_objects";
        {
            let cache = l1();
            let mut guard = cache.lock().expect("锁 L1");
            guard.invalidate_connection(conn_id);
            guard.set_tables(conn_id, "main", Some("public"), vec![table("t1")]);
            guard.set_views(conn_id, "main", Some("public"), vec![]);
            guard.set_columns_detail(conn_id, "main", Some("public"), "t1", vec![column("id")]);
        }

        let svc = offline_service(false);

        let folder = svc
            .load_children(
                conn_id,
                &NavPath::Folder {
                    catalog: "main".to_string(),
                    schema: "public".to_string(),
                    folder: NavFolder::Tables,
                },
            )
            .await
            .expect("表文件夹应从 L1 命中");
        assert!(folder.iter().any(|n| n.name == "t1"), "应含 t1");

        let cols = svc
            .load_children(
                conn_id,
                &NavPath::Table {
                    catalog: "main".to_string(),
                    schema: "public".to_string(),
                    table: "t1".to_string(),
                },
            )
            .await
            .expect("列应从 L1 命中");
        assert_eq!(cols.len(), 1);
        assert_eq!(cols[0].name, "id");
    }

    /// 刷新语义：`fresh` 跳过 L1（以实时内省为准）。
    #[tokio::test]
    async fn fresh_mode_skips_l1() {
        let conn_id = "P_l1_test_fresh";
        {
            let cache = l1();
            let mut guard = cache.lock().expect("锁 L1");
            guard.set_catalogs(conn_id, vec!["main".to_string()]);
        }

        let svc = offline_service(true);
        assert!(
            svc.load_children(conn_id, &NavPath::Connection)
                .await
                .is_err(),
            "刷新模式不得吃 L1，应走实时内省（空连接管理器下必然失败）"
        );
    }

    /// 刷新会清掉该连接的 L1 条目（设计口径：手动刷新 → 清 L1）。
    #[tokio::test]
    async fn refresh_invalidates_l1_for_that_connection() {
        let conn_id = "P_l1_test_invalidate";
        {
            let cache = l1();
            let mut guard = cache.lock().expect("锁 L1");
            guard.set_catalogs(conn_id, vec!["main".to_string()]);
        }

        let svc = offline_service(true);
        let _ = svc.load_children(conn_id, &NavPath::Connection).await;

        let cache = l1();
        let mut guard = cache.lock().expect("锁 L1");
        assert!(
            guard.get_catalogs(conn_id).is_none(),
            "刷新后该连接的 L1 条目应被清掉"
        );
    }
}

#[cfg(test)]
mod paging_tests {
    //! 分页接线测试（C4）：大 schema 走 `metadata_index` 分块，小 schema 保持全量。
    //!
    //! 手法与 `l1_tests` 同理：连接管理器是**空的**，任何真正的实时内省都会失败。
    //! 因此「分页返回 Ok」本身证明这一页是缓存（L2 索引 / 规范化表）里取的，没有回源库。

    use std::sync::Arc;

    use engine::connection_manager::ConnectionManager;
    use engine::driver::traits::{NodeInfo, SchemaObjectKind};
    use engine::persistence::{ConnectionType, MetadataCacheManager, MetadataCachePool};

    use super::*;
    use crate::cache::NavCache;

    /// 大 schema 的对象数（> `CHUNK_THRESHOLD`）。
    const BIG: usize = 600;
    /// 一页的条数（与 UI 首批同量级）。
    const PAGE: usize = 200;

    fn temp_root(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("rds_navpage_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("建临时项目根");
        dir
    }

    fn object(name: String, kind: SchemaObjectKind) -> NodeInfo {
        NodeInfo::new(name, kind)
    }

    /// 在项目缓存里写入一个 schema 与 `count` 个对象（模拟冷启动内省之后的落盘状态）。
    ///
    /// `with_index` 控制是否重建 `metadata_index`：分页测试必须建（分页读的是索引），
    /// 「大 schema 不进 L1」一例故意不建（逼它走 L2 全量回退分支）。
    fn seed(
        conn_id: &str,
        root: &str,
        schema: &str,
        count: usize,
        view: bool,
        with_index: bool,
    ) -> Vec<String> {
        let kind = if view {
            SchemaObjectKind::View
        } else {
            SchemaObjectKind::Table
        };
        let names: Vec<String> = (0..count).map(|i| format!("t{i:04}")).collect();
        {
            let mut cache = NavCache::open(conn_id, Some(root)).expect("打开缓存");
            cache.put_schemas("main", &[schema.to_string()]);
            let sid = cache.schema_id("main", schema).expect("schema_id");
            let objects: Vec<NodeInfo> = names
                .iter()
                .map(|n| object(n.clone(), kind.clone()))
                .collect();
            cache.put_objects(sid, &objects);
            if with_index {
                cache.rebuild_index("main", schema);
            }
        }
        names
    }

    fn service(root: &str, fresh: bool) -> NavigatorService {
        NavigatorService::with_context(
            Arc::new(ConnectionManager::new()),
            Some(root.to_string()),
            fresh,
        )
    }

    fn folder(schema: &str, folder: NavFolder) -> NavPath {
        NavPath::Folder {
            catalog: "main".to_string(),
            schema: schema.to_string(),
            folder,
        }
    }

    fn cleanup(root: &std::path::Path, conn_id: &str) {
        // 先丢池（Windows 上句柄不释放就删不掉文件），再删临时根。
        if let Ok(manager) = MetadataCacheManager::new(
            conn_id,
            ConnectionType::Project,
            Some(root.to_string_lossy().as_ref()),
        ) {
            MetadataCachePool::drop_pool(manager.db_path());
        }
        let _ = std::fs::remove_dir_all(root);
    }

    /// 计数来源：只有「表」能信索引计数。
    ///
    /// 反例场景（曾经真会出错）：索引里只有表行、而 L2 里有视图
    /// （先展开表、以后再展开视图——视图那趟不重建索引），
    /// 此时若照索引的 `view_count = 0` 判定，视图文件夹会整支从树上消失。
    #[test]
    fn only_tables_trust_index_counts() {
        let counts = SchemaObjectCounts {
            table_count: 7,
            view_count: 0,
            column_count: 3,
            routine_count: 0,
            total: 10,
        };
        assert_eq!(
            NavigatorService::indexed_folder_count(NavFolder::Tables, Some(&counts)),
            Some(7)
        );
        assert_eq!(
            NavigatorService::indexed_folder_count(NavFolder::Views, Some(&counts)),
            None,
            "视图不得照索引计数判定（索引缺 view 行时会把文件夹隐藏）"
        );
        for folder in [
            NavFolder::Routines,
            NavFolder::Sequences,
            NavFolder::Triggers,
        ] {
            assert_eq!(
                NavigatorService::indexed_folder_count(folder, Some(&counts)),
                None,
                "{folder:?} 不在索引里，必须全量收集"
            );
        }
        // 索引不可用（冷启动）→ 一律回退全量
        assert_eq!(
            NavigatorService::indexed_folder_count(NavFolder::Tables, None),
            None
        );
    }

    /// 大 schema 首屏：只取一页，`total` 告知真实总数（客户端分页靠它才有“余量”）。
    #[tokio::test]
    async fn large_schema_first_page_comes_from_index() {
        let root = temp_root("first_page");
        let root_s = root.to_string_lossy().to_string();
        let conn_id = "P_page_first";
        seed(conn_id, &root_s, "public", BIG, false, true);

        let page = service(&root_s, false)
            .load_children_page(
                conn_id,
                &folder("public", NavFolder::Tables),
                0,
                PAGE,
            )
            .await
            .expect("应从 L2 索引分页，不该回源库");
        assert_eq!(page.nodes.len(), PAGE, "首屏只取一页");
        assert_eq!(page.total, BIG, "总数来自索引计数");
        assert!(page.has_more, "600 条取 200 条应报告还有后续");
        assert_eq!(page.offset, 0);
        // 索引顺序稳定（名称升序）：翻页不能有随机顺序
        assert_eq!(page.nodes[0].name, "t0000");
        assert_eq!(page.nodes[PAGE - 1].name, "t0199");

        cleanup(&root, conn_id);
    }

    /// 定位：位次 → 页起点 → 那一页里**真有目标**。
    ///
    /// 这是「搜索命中在树中定位」在大 schema 上的生死线：位次算错一，
    /// 跳过去的那一页里就没有它，而界面会说「已定位」（比不定位更糟）。
    #[tokio::test]
    async fn object_position_lands_on_a_page_containing_the_target() {
        let root = temp_root("locate_position");
        let root_s = root.to_string_lossy().to_string();
        let conn_id = "P_page_locate";
        seed(conn_id, &root_s, "public", BIG, false, true);
        let svc = service(&root_s, false);

        let path = folder("public", NavFolder::Tables);
        // 末尾那一条最考验人：位次 599，页起点应是 400（按页向下取整）
        let position = svc
            .object_position(conn_id, &path, "t0599")
            .expect("t0599 应在索引里");
        assert_eq!(position, BIG - 1);

        let page_size = PAGE;
        let offset = position - (position % page_size);
        let page = svc
            .load_children_page(conn_id, &path, offset, page_size)
            .await
            .expect("应从 L2 索引取页");
        assert!(
            page.nodes.iter().any(|n| n.name == "t0599"),
            "按位次取的那一页里必须有目标（否则定位会停在别处）"
        );

        // 索引里没有的对象：绝不能退化成「位次 0」（那会把人送到第一页并声称定位成功）
        assert_eq!(svc.object_position(conn_id, &path, "不存在"), None);
        // 不分页的类别不做定位（它们的行总是全量加载，直接选中即可）
        assert_eq!(
            svc.object_position(conn_id, &folder("public", NavFolder::Routines), "t0000"),
            None
        );
        // 非文件夹路径不猜位次
        assert_eq!(
            svc.object_position(
                conn_id,
                &NavPath::Schema {
                    catalog: "main".into(),
                    schema: "public".into(),
                },
                "public"
            ),
            None
        );

        cleanup(&root, conn_id);
    }

    /// 第二页与首屏不重叠；翻到底后 `has_more` 归 false。
    #[tokio::test]
    async fn second_page_is_disjoint_and_last_page_reports_no_more() {
        let root = temp_root("second_page");
        let root_s = root.to_string_lossy().to_string();
        let conn_id = "P_page_second";
        seed(conn_id, &root_s, "public", BIG, false, true);
        let svc = service(&root_s, false);

        let first = svc
            .load_children_page(conn_id, &folder("public", NavFolder::Tables), 0, PAGE)
            .await
            .expect("首屏");
        let second = svc
            .load_children_page(conn_id, &folder("public", NavFolder::Tables), PAGE, PAGE)
            .await
            .expect("第二页");
        assert_eq!(second.nodes.len(), PAGE);
        assert_eq!(second.nodes[0].name, "t0200", "第二页应从第 200 条继续");
        assert!(
            second
                .nodes
                .iter()
                .all(|n| !first.nodes.iter().any(|f| f.key == n.key)),
            "两页不得重叠"
        );

        let last = svc
            .load_children_page(conn_id, &folder("public", NavFolder::Tables), 2 * PAGE, PAGE)
            .await
            .expect("末页");
        assert_eq!(last.nodes.len(), BIG - 2 * PAGE);
        assert!(!last.has_more, "最后一块不得再报告有余量");

        cleanup(&root, conn_id);
    }

    /// 视图文件夹同样能分页（引擎侧原实现把 `object_type='table'` 写死，视图读不到）。
    #[tokio::test]
    async fn views_folder_pages_from_index() {
        let root = temp_root("views");
        let root_s = root.to_string_lossy().to_string();
        let conn_id = "P_page_views";
        seed(conn_id, &root_s, "public", BIG, true, true);

        let page = service(&root_s, false)
            .load_children_page(conn_id, &folder("public", NavFolder::Views), 0, PAGE)
            .await
            .expect("视图文件夹应能从索引分页");
        assert_eq!(page.nodes.len(), PAGE);
        assert_eq!(page.total, BIG);
        assert!(page.has_more);
        assert!(
            page.nodes
                .iter()
                .all(|n| matches!(n.kind, NavNodeKind::View)),
            "视图页的节点类型必须是视图"
        );

        cleanup(&root, conn_id);
    }

    /// 小 schema 不进入分块路径：页就是全量，也没有「加载更多」。
    #[tokio::test]
    async fn small_schema_returns_all_without_more() {
        let root = temp_root("small");
        let root_s = root.to_string_lossy().to_string();
        let conn_id = "P_page_small";
        seed(conn_id, &root_s, "public", 3, false, true);

        let page = service(&root_s, false)
            .load_children_page(conn_id, &folder("public", NavFolder::Tables), 0, PAGE)
            .await
            .expect("小 schema 应从 L2 全量返回");
        assert_eq!(page.nodes.len(), 3);
        assert_eq!(page.total, 3);
        assert!(!page.has_more);

        cleanup(&root, conn_id);
    }

    /// 刷新（`fresh`）不得吃索引：索引是刷新前的旧数据，必须回源实时内省
    /// （空连接管理器下必然失败——这正是本条要断言的）。
    #[tokio::test]
    async fn fresh_mode_ignores_index() {
        let root = temp_root("fresh");
        let root_s = root.to_string_lossy().to_string();
        let conn_id = "P_page_fresh";
        seed(conn_id, &root_s, "public", BIG, false, true);

        assert!(
            service(&root_s, true)
                .load_children_page(conn_id, &folder("public", NavFolder::Tables), 0, PAGE)
                .await
                .is_err(),
            "刷新模式必须以实时内省为准，不得直接返回索引里的旧列表"
        );

        cleanup(&root, conn_id);
    }

    /// 大 schema 不进 L1：L2 有 600 行、索引缺失时走全量回退，
    /// 但结果**不得**写进进程内 L1（否则万级对象会常驻内存）。
    #[tokio::test]
    async fn large_schema_is_not_kept_in_l1() {
        let root = temp_root("no_l1");
        let root_s = root.to_string_lossy().to_string();
        let conn_id = "P_page_no_l1";
        // 故意不建索引：逼它走「L2 全量」回退分支（模拟索引尚未重建的窗口）。
        seed(conn_id, &root_s, "public", BIG, false, false);

        let svc = service(&root_s, false);
        let page = svc
            .load_children_page(conn_id, &folder("public", NavFolder::Tables), 0, PAGE)
            .await
            .expect("索引缺失时应回退 L2 全量（仍不应回源库）");
        assert_eq!(page.total, BIG);
        assert!(!page.has_more, "全量回退路径是一次拉全");

        let l1 = engine::cache::CacheManager::instance()
            .lock()
            .expect("CacheManager 单例")
            .metadata_cache();
        let mut guard = l1.lock().expect("锁 L1");
        assert!(
            guard
                .get_tables(conn_id, "main", Some("public"))
                .is_none(),
            "大 schema 不得驻留 L1（万级对象会一直占着内存）"
        );

        cleanup(&root, conn_id);
    }
}

#[cfg(test)]
mod cached_folder_tests {
    //! 例程 / 序列 / 触发器的 L2 命中（2026-09-19 接线）。
    //!
    //! 手法同 `paging_tests`：连接管理器是**空的**，任何实时内省都会失败——
    //! 所以「返回 Ok」本身证明结果来自缓存，没有回源库。

    use std::sync::Arc;

    use engine::cache::CacheManager;
    use engine::connection_manager::ConnectionManager;
    use engine::driver::traits::{NodeInfo, SchemaObjectKind};
    use engine::persistence::{ConnectionType, MetadataCacheManager, MetadataCachePool};

    use super::*;
    use crate::cache::NavCache;

    fn temp_root(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("rds_navfolder_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("建临时项目根");
        dir
    }

    /// 空连接管理器的服务；先清该连接的 L1（进程内单例在测试间共享，不清会串味）。
    fn service(conn_id: &str, root: &str, fresh: bool) -> NavigatorService {
        if let Ok(manager) = CacheManager::instance().lock() {
            manager.invalidate_connection(conn_id);
        }
        NavigatorService::with_context(
            Arc::new(ConnectionManager::new()),
            Some(root.to_string()),
            fresh,
        )
    }

    fn folder(schema: &str, folder: NavFolder) -> NavPath {
        NavPath::Folder {
            catalog: "main".to_string(),
            schema: schema.to_string(),
            folder,
        }
    }

    fn cleanup(root: &std::path::Path, conn_id: &str) {
        // 先丢池（Windows 上句柄不释放就删不掉文件），再删临时根。
        if let Ok(manager) = MetadataCacheManager::new(
            conn_id,
            ConnectionType::Project,
            Some(root.to_string_lossy().as_ref()),
        ) {
            MetadataCachePool::drop_pool(manager.db_path());
        }
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn routines_come_from_l2_without_touching_the_source() {
        let conn_id = "P_folder_routines";
        let root = temp_root("routines");
        let root_s = root.to_string_lossy().to_string();
        {
            let cache = NavCache::open(conn_id, Some(&root_s)).expect("打开缓存");
            cache.put_schemas("main", &["public".to_string()]);
            let sid = cache.schema_id("main", "public").expect("schema_id");
            cache.put_routines(
                sid,
                &[
                    NodeInfo::new("refresh_stats", SchemaObjectKind::Procedure),
                    NodeInfo::new("order_total", SchemaObjectKind::Function),
                ],
            );
        }

        let svc = service(conn_id, &root_s, false);
        let nodes = svc
            .load_children(conn_id, &folder("public", NavFolder::Routines))
            .await
            .expect("例程文件夹应从 L2 命中");

        let names: Vec<&str> = nodes.iter().map(|n| n.name.as_str()).collect();
        assert!(names.contains(&"refresh_stats"), "{names:?}");
        assert!(names.contains(&"order_total"), "{names:?}");
        // 类别往返：过程 / 函数在 L2 用 `routine_type` 区分，回读不得混成一类。
        let kinds: Vec<String> = nodes
            .iter()
            .filter_map(|n| match &n.kind {
                NavNodeKind::Routine { routine_type } => Some(routine_type.clone()),
                _ => None,
            })
            .collect();
        assert!(kinds.contains(&"Procedure".to_string()), "{kinds:?}");
        assert!(kinds.contains(&"Function".to_string()), "{kinds:?}");

        cleanup(&root, conn_id);
    }

    #[tokio::test]
    async fn sequences_come_from_l2_without_touching_the_source() {
        let conn_id = "P_folder_sequences";
        let root = temp_root("sequences");
        let root_s = root.to_string_lossy().to_string();
        {
            let cache = NavCache::open(conn_id, Some(&root_s)).expect("打开缓存");
            cache.put_schemas("main", &["public".to_string()]);
            let sid = cache.schema_id("main", "public").expect("schema_id");
            cache.put_sequences(sid, &[NodeInfo::new("order_seq", SchemaObjectKind::Sequence)]);
        }

        let svc = service(conn_id, &root_s, false);
        let nodes = svc
            .load_children(conn_id, &folder("public", NavFolder::Sequences))
            .await
            .expect("序列文件夹应从 L2 命中");

        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].name, "order_seq");

        cleanup(&root, conn_id);
    }

    /// 触发器连**所属表**一起往返（`NodeInfo::parent_name` ↔ `triggers.table_id`）。
    ///
    /// 这条不只是显示问题：`triggers.table_id` 是 `NOT NULL`，所属表丢了就等于这条
    /// 触发器根本存不进去（写侧会跳过它）——所以这里断言的是「能不能缓存」本身。
    #[tokio::test]
    async fn triggers_come_from_l2_and_keep_their_table() {
        let conn_id = "P_folder_triggers";
        let root = temp_root("triggers");
        let root_s = root.to_string_lossy().to_string();
        {
            let cache = NavCache::open(conn_id, Some(&root_s)).expect("打开缓存");
            cache.put_schemas("main", &["public".to_string()]);
            let sid = cache.schema_id("main", "public").expect("schema_id");
            cache.put_triggers(
                sid,
                &[NodeInfo::new("audit_trg", SchemaObjectKind::Trigger).with_parent("orders")],
            );
        }

        let svc = service(conn_id, &root_s, false);
        let nodes = svc
            .load_children(conn_id, &folder("public", NavFolder::Triggers))
            .await
            .expect("触发器文件夹应从 L2 命中");

        assert_eq!(nodes.len(), 1);
        assert_eq!(
            nodes[0].property.as_ref().and_then(|p| p.parent.as_deref()),
            Some("orders"),
            "所属表要跟着触发器一起往返"
        );

        cleanup(&root, conn_id);
    }

    /// 刷新模式不得吃这三类缓存（与表 / 视图同一口径：刷新以实时内省为准）。
    #[tokio::test]
    async fn fresh_mode_ignores_the_folder_cache() {
        let conn_id = "P_folder_fresh";
        let root = temp_root("fresh");
        let root_s = root.to_string_lossy().to_string();
        {
            let cache = NavCache::open(conn_id, Some(&root_s)).expect("打开缓存");
            cache.put_schemas("main", &["public".to_string()]);
            let sid = cache.schema_id("main", "public").expect("schema_id");
            cache.put_sequences(sid, &[NodeInfo::new("order_seq", SchemaObjectKind::Sequence)]);
        }

        let svc = service(conn_id, &root_s, true);
        assert!(
            svc.load_children(conn_id, &folder("public", NavFolder::Sequences))
                .await
                .is_err(),
            "刷新模式应走实时内省（空连接管理器下必然失败）"
        );

        cleanup(&root, conn_id);
    }
}

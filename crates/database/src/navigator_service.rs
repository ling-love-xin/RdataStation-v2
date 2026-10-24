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

/// 表列内联渲染阈值（原型 §11 #6）：列数超过该值不再逐列生成树节点，
/// 改由单个 [`NavNodeKind::ColumnOverflow`] 提示节点跳属性面板「列」页签。
pub const MAX_INLINE_COLUMNS: usize = 50;

use engine::cache::{CacheManager, MetadataCache};
use engine::connection_manager::ConnectionManager;
use engine::driver::traits::{ColumnDetail, NodeInfo, SchemaObjectKind, SchemaObjects};
use engine::persistence::SchemaObjectCounts;
use shared::error::CoreError;

use crate::cache::NavCache;
use crate::metadata_service::MetadataService;
use crate::model::{
    NavFolder, NavNode, NavNodeKind, NavPath, NavSource, PropertyRef,
};
use property::PropertyKind;

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

/// 系统 catalog 判定（S3-①，fluxDB `tree_helpers.rs:1281-1286` 同清单，大小写不敏感）。
///
/// 命中即默认隐藏（未设置可见库白名单时）；白名单模式原样展示。
/// `main`（FALLBACK_CONTAINER 与 SQLite/DuckDB 的单一库）不在清单里，永不隐藏。
pub fn nav_is_system_catalog(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "information_schema" | "mysql" | "performance_schema" | "sys" | "admin" | "local"
    )
}

/// 系统 schema 判定（S3-①）：PG 侧系统 schema 与临时 schema 前缀。
///
/// `main` 与普通用户 schema 不在清单里。MySQL / SQLite / DuckDB 无 schema 层，
/// 该函数只对 PG（及未来支持 schema 层的驱动）生效。
pub fn nav_is_system_schema(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    matches!(
        lower.as_str(),
        "pg_catalog" | "pg_toast" | "information_schema"
    ) || lower.starts_with("pg_temp_")
        || lower.starts_with("pg_toast_temp_")
}

/// S3-① 可见性过滤：`None`（默认模式）= 隐藏系统库 / 系统 schema；
/// `Some(集合)`（白名单模式）= 只保留集合里的名字（含系统对象，原样展示）。
fn nav_keep_object(name: &str, visible: Option<&std::collections::BTreeSet<String>>, is_system: fn(&str) -> bool) -> bool {
    match visible {
        Some(set) => set.contains(name),
        None => !is_system(name),
    }
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
    ///
    /// `visible`（S3-①）：`None` = 默认模式隐藏系统库 / 系统 schema；
    /// `Some(集合)` = 白名单模式（只保留集合里的名字，含系统对象）。
    pub async fn load_children(
        &self,
        conn_id: &str,
        path: &NavPath,
        visible: Option<&std::collections::BTreeSet<String>>,
    ) -> Result<Vec<NavNode>, CoreError> {
        // 刷新语义：以实时内省为准，先清该连接的 L1（L2 由各路径重写）。
        if self.fresh {
            self.l1_invalidate_connection(conn_id);
        }

        match path {
            NavPath::Connection => self.load_catalogs(conn_id, visible).await,
            NavPath::Catalog { catalog } => {
                // 层级按数据库类型动态决定：无独立 Schema 层的驱动（MySQL / SQLite /
                // DuckDB）让 Catalog 直接承载类别文件夹，避免出现同名重复的 Schema 层。
                if self.metadata.has_schema_level(conn_id).await? {
                    self.load_schemas(conn_id, catalog, visible).await
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
        visible: Option<&std::collections::BTreeSet<String>>,
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
                let nodes = self.load_children(conn_id, path, visible).await?;
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
        // 独立路径无共享 batch：临时 None → 单次批量内省（一条连接拿全部类别）。
        let mut batch = None;
        let objects = self
            .collect_objects(conn_id, catalog, schema, folder, &mut batch)
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
    async fn load_catalogs(
        &self,
        conn_id: &str,
        visible: Option<&std::collections::BTreeSet<String>>,
    ) -> Result<Vec<NavNode>, CoreError> {
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
            // S3-① 可见性过滤：默认隐藏系统库；白名单模式只留白名单。
            catalogs
                .into_iter()
                .filter(|name| nav_keep_object(name, visible, nav_is_system_catalog))
                .collect::<Vec<_>>()
        };
        // 过滤后为空（全部是系统库且默认模式）也要有落脚点：保底给单一容器，
        // 树仍可下钻（MySQL 的 information_schema 等全隐藏后至少能看到一个容器）。
        let names = if names.is_empty() {
            vec![FALLBACK_CONTAINER.to_string()]
        } else {
            names
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
    async fn load_schemas(
        &self,
        conn_id: &str,
        catalog: &str,
        visible: Option<&std::collections::BTreeSet<String>>,
    ) -> Result<Vec<NavNode>, CoreError> {
        // 部分驱动（SQLite/DuckDB）不区分 catalog/schema，查询可能返回空；
        // 空结果按单一 `main` schema 处理，保证树仍可下钻。
        let names = self.schema_names(conn_id, catalog).await?;
        // S3-① schema 层可见性：默认（`visible = None`）隐藏 PG 系统 schema
        // （pg_catalog / pg_toast / information_schema / pg_temp_*）。
        // 白名单模式：所属 catalog 已被勾选（整库放行，含系统 schema——用户明确要
        // 显示这个库）或该 schema 名本身在白名单里，才保留。
        let names = names
            .into_iter()
            .filter(|name| {
                let in_visible_catalog =
                    visible.is_some_and(|set| set.contains(catalog));
                in_visible_catalog || nav_keep_object(name, visible, nav_is_system_schema)
            })
            .collect::<Vec<_>>();
        let names = if names.is_empty() {
            vec![FALLBACK_CONTAINER.to_string()]
        } else {
            names
        };
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

    /// 可见库管理弹框（S3-①b）用：**全量** catalog 清单（不过滤系统对象）。
    ///
    /// 与 [`Self::load_catalogs`] 的区别只在于不应用 `nav_is_system_catalog` 过滤——
    /// 弹框要列出全部库供勾选（含系统库，由「显示系统对象」开关控制展示）。
    /// 同样走 L1 → 实时内省（Catalog 不在 L2 缓存范围），有缓存时零 I/O。
    pub async fn list_all_catalogs(&self, conn_id: &str) -> Result<Vec<String>, CoreError> {
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
        Ok(names)
    }

    /// 可见库管理弹框（S3-①b）用：某 catalog 下**全量** schema 清单（不过滤系统 schema）。
    ///
    /// 与 [`Self::load_schemas`] 的区别只在于不应用 `nav_is_system_schema` 过滤。
    pub async fn list_all_schemas(
        &self,
        conn_id: &str,
        catalog: &str,
    ) -> Result<Vec<String>, CoreError> {
        let names = self.schema_names(conn_id, catalog).await?;
        let names = if names.is_empty() {
            vec![FALLBACK_CONTAINER.to_string()]
        } else {
            names
        };
        Ok(names)
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

    /// Schema → 类别文件夹（DBeaver 分块模型：**静态文件夹，不内省对象**）。
    ///
    /// 2026-10-08 顺滑度三阶段（真机铁证）：本环境（192.168.3.138 链路）**跨任务连接
    /// 复用必挂**（app_run68：biz_484 connect_ms=2176 / biz_495 connect_ms=2647 = 每次
    /// 展开 ping 2s 超时 + 重建——复用反而更慢）。因此展开 schema **完全不查库**：
    /// 只渲染 5 个静态类别文件夹（Tables/Views/Routines/Sequences/Triggers），毫秒级
    /// 瞬时；对象列表在**点击类别文件夹**时才加载（collect_objects：冷展开一次
    /// 300ms 可接受、L2 命中毫秒级）——这正是 DBeaver 的树交互（展开 schema 出骨架、
    /// 点类别才查对象）。
    async fn load_folders(
        &self,
        conn_id: &str,
        catalog: &str,
        schema: &str,
    ) -> Result<Vec<NavNode>, CoreError> {
        tracing::info!(conn_id = %conn_id, catalog = %catalog, schema = %schema, "导航 load_folders 开始（静态类别文件夹）");
        // 空类别也显示（用户决策 2026-10-08：所有类别固定显示，点击空类别显示空）。
        Ok(NavFolder::ALL
            .iter()
            .map(|folder| Self::folder_node(conn_id, catalog, schema, *folder, None))
            .collect())
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

    /// 类别文件夹节点（DBeaver 分块模型：静态渲染，无计数——对象数在点击文件夹
    /// 加载后才知；`count=None` 只显示类别名）。
    fn folder_node(
        conn_id: &str,
        catalog: &str,
        schema: &str,
        folder: NavFolder,
        count: Option<usize>,
    ) -> NavNode {
        let title = match count {
            Some(c) => format!("{} ({})", folder.label(), c),
            None => folder.label().to_string(),
        };
        NavNode::new(
            NavNode::child_key(conn_id, &[catalog, schema, folder.key()]),
            title,
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
        // 独立路径无共享 batch：临时 None → 单次批量内省（一条连接拿全部类别）。
        let mut batch = None;
        let objects = self
            .collect_objects(conn_id, catalog, schema, folder, &mut batch)
            .await?;
        Ok(Self::object_nodes(conn_id, catalog, schema, folder, objects))
    }

    /// 对象 → 导航节点（分页路径与全量路径共用，保证同一个对象在两路上长得一样）。
    ///
    /// S3-② 分区/继承折叠：有 `parent_name` 的表 = 子表（分区或继承），**不单独列出**，
    /// 折叠进父表——父表行尾徽标显示「分区 N / 继承 N」。折叠先于分页，保证
    /// 父表无论落在哪一页，计数都是该 schema 的全量（与 blanco 的 `ChildTables` 同形）。
    fn object_nodes(
        conn_id: &str,
        catalog: &str,
        schema: &str,
        folder: NavFolder,
        objects: Vec<NodeInfo>,
    ) -> Vec<NavNode> {
        // 先统计父表计数（子表按 parent_name 归并），再逐对象产出节点。
        // 计数 map 用 owned String 键：`&str` 键会借用 `objects`，后续 `into_iter()`
        // 无法 move（E0505）。
        let (child_partitions, child_inherited) = if folder == NavFolder::Tables {
            let mut part: std::collections::BTreeMap<String, u32> = Default::default();
            let mut inh: std::collections::BTreeMap<String, u32> = Default::default();
            for obj in &objects {
                if let Some(p) = obj.parent_name.as_deref() {
                    if obj.is_partition {
                        *part.entry(p.to_string()).or_default() += 1;
                    } else {
                        *inh.entry(p.to_string()).or_default() += 1;
                    }
                }
            }
            (part, inh)
        } else {
            (Default::default(), Default::default())
        };

        objects
            .into_iter()
            .filter_map(|obj| {
                // 子表不产出节点（折叠进父表徽标）。
                if folder == NavFolder::Tables && obj.parent_name.is_some() {
                    return None;
                }
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
                let mut node = NavNode::new(key, name.clone(), conn_id, kind, has_children)
                    .with_comment(obj.comment)
                    .with_property(kind_prop);
                if has_children {
                    node = node.with_expand_path(NavPath::Table {
                        catalog: catalog.to_string(),
                        schema: schema.to_string(),
                        table: name.clone(),
                    });
                }
                // 父表带上折叠计数（两半都为零时也成立——普通表不带徽标）。
                if folder == NavFolder::Tables {
                    let partitions = child_partitions.get(&name).copied().unwrap_or(0);
                    let inherited = child_inherited.get(&name).copied().unwrap_or(0);
                    node = node.with_child_counts(partitions, inherited);
                }
                Some(node)
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
        // 大 schema 阈值（原型 §11 #6）：列数超限不内联渲染（几十列会淹没树），
        // 生成单个提示节点跳表属性面板——属性面板已有「列 (N)」页签（`PageId::Columns`）。
        if columns.len() > MAX_INLINE_COLUMNS {
            let key = NavNode::child_key(conn_id, &[catalog, schema, table, "~columns"]);
            let prop = PropertyRef {
                conn_id: conn_id.to_string(),
                source: NavSource::from_conn_id(conn_id),
                catalog: Some(catalog.to_string()),
                schema: Some(schema.to_string()),
                parent: None,
                name: table.to_string(),
                kind: PropertyKind::Table,
            };
            return vec![NavNode::new(
                key,
                format!("在属性面板查看列（{} 列）", columns.len()),
                conn_id,
                NavNodeKind::ColumnOverflow,
                false,
            )
            .with_property(prop)];
        }
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
    ///
    /// `batch` 是本次 `load_folders` 的批量内省缓存：某类别未命中 L1/L2 时，
    /// 若 batch 为空则先调 `introspect_schema_objects`（一次建连拿全部类别），
    /// 再从中取当前类别——一次展开至多一次建连，且无「同 SQL 二次执行」。
    async fn collect_objects(
        &self,
        conn_id: &str,
        catalog: &str,
        schema: &str,
        folder: NavFolder,
        batch: &mut Option<SchemaObjects>,
    ) -> Result<Vec<NodeInfo>, CoreError> {
        match folder {
            NavFolder::Tables | NavFolder::Views => {
                let want_view = folder == NavFolder::Views;
                let tc = std::time::Instant::now();

                // L1：表与视图分键（下面一次实时内省会两半都写进去）
                if let Some(objs) = self.l1_read(|c| {
                    if want_view {
                        c.get_views(conn_id, catalog, Some(schema))
                    } else {
                        c.get_tables(conn_id, catalog, Some(schema))
                    }
                }) {
                    tracing::info!(conn_id = %conn_id, folder = folder.key(), elapsed_ms = tc.elapsed().as_millis(), "collect_objects L1 命中");
                    return Ok(objs);
                }

                let mut cache = self.cache(conn_id);
                tracing::info!(conn_id = %conn_id, folder = folder.key(), elapsed_ms = tc.elapsed().as_millis(), "collect_objects 缓存句柄就绪");
                let mut schema_id = cache.as_ref().and_then(|c| c.schema_id(catalog, schema));
                if !self.fresh {
                    if let (Some(c), Some(sid)) = (&cache, schema_id) {
                        if let Some(rows) = c.objects(sid, want_view) {
                            let objs: Vec<NodeInfo> = rows
                                .into_iter()
                                .map(|(name, comment, parent, is_partition)| {
                                    let mut node = NodeInfo::new(
                                        name,
                                        if want_view {
                                            SchemaObjectKind::View
                                        } else {
                                            SchemaObjectKind::Table
                                        },
                                    )
                                    .with_comment(comment);
                                    // S3-②：L2 命中还原子表的父表与分区标记（折叠结果不丢）。
                                    if let Some(p) = parent {
                                        node = node.with_parent(p);
                                        if is_partition {
                                            node = node.with_partition();
                                        }
                                    }
                                    node
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
                            // 读侧自愈写侧（幂等）：命中即证明对象写入完整，补扫描完成标记，
                            // 让 `is_schema_complete` 守卫在此后「空类别」判断中生效
                            // （schema 列表已全量显示，未完整扫描的 schema 靠这个标记展开时回源）。
                            c.mark_schema_complete(catalog, schema);
                            return Ok(objs);
                        } else if c.is_schema_complete(catalog, schema) {
                            // schema 已完整扫描、但该类别确实为空（如 MySQL 无视图）：
                            // 空是合法结果，不是「未命中」——直接返回空，避免每次展开都回源
                            // 实时查询（真机：views 空 + 半成品时反复回源 get_tables，池不稳时 30s 超时）。
                            tracing::info!(conn_id = %conn_id, folder = folder.key(), elapsed_ms = tc.elapsed().as_millis(), "collect_objects 完整 schema 空类别（不回源）");
                            return Ok(Vec::new());
                        }
                    }
                }
                let objects = {
                    // 批量内省：一次展开至多一次建连（DBeaver 会话语义）；之后各类别共享。
                    if batch.is_none() {
                        *batch = Some(
                            self.metadata
                                .introspect_schema_objects(conn_id, catalog, schema)
                                .await?,
                        );
                    }
                    batch.as_ref().expect("刚初始化").tables.clone()
                };
                tracing::info!(conn_id = %conn_id, folder = folder.key(), elapsed_ms = tc.elapsed().as_millis(), count = objects.len(), "collect_objects 实时内省完成");
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
                    // 清旧行时机：刷新（fresh）**或**该 schema 是半成品（未完成扫描）。
                    // 半成品（进程中途退出留下「有 schema 行、对象不完整」）重扫时必须
                    // 先清残留对象再整块重写，否则已删的表会留在缓存里被读到。
                    // 已完整的 schema 不 prune：表趟写完视图趟只补视图，清旧行会误删表数据。
                    if self.fresh || !c.is_schema_complete(catalog, schema) {
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
                    // 扫描完成标记（幂等）：表/视图写入 + 索引重建完成后置 is_loaded=1。
                    // 半成品防御的写侧一半（读侧 `schemas()` 过滤未完成的 schema）。
                    c.mark_schema_complete(catalog, schema);
                }
                Ok(filtered)
            }
            NavFolder::Routines => {
                tracing::info!(conn_id = %conn_id, catalog = %catalog, schema = %schema, "collect_objects 类别进入 routines");
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
                let mut procedures = {
                    if batch.is_none() {
                        *batch = Some(
                            self.metadata
                                .introspect_schema_objects(conn_id, catalog, schema)
                                .await?,
                        );
                    }
                    batch.as_ref().expect("刚初始化").procedures.clone()
                };
                let functions = batch.as_ref().expect("刚初始化").functions.clone();
                procedures.extend(functions);
                if let (Some(c), Some(sid)) = (cache.as_ref(), schema_id) {
                    c.put_routines(sid, &procedures);
                }
                self.l1_write_routines(conn_id, catalog, schema, procedures.clone());
                Ok(procedures)
            }
            NavFolder::Sequences => {
                tracing::info!(conn_id = %conn_id, catalog = %catalog, schema = %schema, "collect_objects 类别进入 sequences");
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
                let objs = {
                    if batch.is_none() {
                        *batch = Some(
                            self.metadata
                                .introspect_schema_objects(conn_id, catalog, schema)
                                .await?,
                        );
                    }
                    batch.as_ref().expect("刚初始化").sequences.clone()
                };
                if let (Some(c), Some(sid)) = (cache.as_ref(), schema_id) {
                    c.put_sequences(sid, &objs);
                }
                let store = objs.clone();
                self.l1_write(move |l1| l1.set_sequences(conn_id, catalog, Some(schema), store));
                Ok(objs)
            }
            NavFolder::Triggers => {
                tracing::info!(conn_id = %conn_id, catalog = %catalog, schema = %schema, "collect_objects 类别进入 triggers");
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
                let objs = {
                    if batch.is_none() {
                        *batch = Some(
                            self.metadata
                                .introspect_schema_objects(conn_id, catalog, schema)
                                .await?,
                        );
                    }
                    batch.as_ref().expect("刚初始化").triggers.clone()
                };
                if let (Some(c), Some(sid)) = (cache.as_ref(), schema_id) {
                    c.put_triggers(sid, &objs);
                }
                let store = objs.clone();
                self.l1_write(move |l1| l1.set_triggers(conn_id, catalog, Some(schema), store));
                Ok(objs)
            }
        }
    }

    /// DBeaver `loadChildren` 对齐（2026-10-09）：**schema 展开即后台全量内省**。
    ///
    /// 用户点开 schema 的瞬间拿到静态类别文件夹（0ms，`load_folders`）；此后点
    /// Tables / Views / Routines / … 时本方法已完成内省并写缓存 → 命中 0ms，
    /// 不再为每次点击付「建连 + SQL」的等待（此前冷展开 384ms~1.4s 的等待感来源）。
    ///
    /// - 幂等：`is_schema_complete` 的 schema 直接跳过（数据已完整在 L2）；
    /// - 失败不阻塞：仅记日志，用户点击时仍走 `collect_objects` 实时回源兜底；
    /// - 与 `collect_objects` 同一批写入口径（L1 小 schema 全类别；L2 全类别 +
    ///   索引重建 + 完成标记），预热与点击并发时写入幂等。
    pub async fn warm_schema(&self, conn_id: &str, catalog: &str, schema: &str) {
        // 完整性判定（2026-10-09 批量列）：对象完整与列完整分开记——旧缓存 schema
        // （对象完整但列未预热，如 8 表阈值时代的大 schema）展开时仍补列。
        let (objects_complete, columns_done) = match self.cache(conn_id) {
            Some(c) => (
                c.is_schema_complete(catalog, schema),
                c.schema_columns_count(catalog, schema) > 0,
            ),
            None => (false, false),
        };
        if objects_complete && columns_done {
            return;
        }
        let t = std::time::Instant::now();
        // 对象来源：不完整 → 实时内省一次（对象段 + 列段共用）；完整 → L2 读
        // 表/视图名（列段用，不重跑内省）。
        let mut objects = None;
        let mut l2_names: Vec<String> = Vec::new();
        if !objects_complete {
            match self
                .metadata
                .introspect_schema_objects(conn_id, catalog, schema)
                .await
            {
                Ok(o) => objects = Some(o),
                Err(e) => {
                    tracing::warn!(conn_id, catalog, schema, err = %e, "后台预热内省失败（点击时仍会实时回源）");
                    return;
                }
            }
        } else {
            l2_names = self
                .cache(conn_id)
                .map(|c| c.schema_object_names(catalog, schema))
                .unwrap_or_default();
        }
        // 对象段：仅不完整时内省 + 写 L1/L2（完整时跳过——数据已在 L2）
        let (tables_half, views_half, table_count) = if !objects_complete {
            let objects = objects.as_ref().expect("内省对象必然存在");
            let table_count = objects.tables.len();
            let (tables_half, views_half): (Vec<NodeInfo>, Vec<NodeInfo>) = objects
                .tables
                .clone()
                .into_iter()
                .partition(|o| o.kind != SchemaObjectKind::View);
            // 例程合并（过程 + 函数），与 collect_objects 的 routines 分支同口径
            let mut routines_all = objects.procedures.clone();
            routines_all.extend(objects.functions.clone());
            // L1：小 schema 全类别进内存（大 schema 走索引分块，不写 L1）
            if table_count <= CHUNK_THRESHOLD {
                let (t, v) = (tables_half.clone(), views_half.clone());
                let (s, g) = (objects.sequences.clone(), objects.triggers.clone());
                let (cid, cat, sch) = (conn_id.to_string(), catalog.to_string(), schema.to_string());
                let (cid_c, cat_c, sch_c) = (cid.clone(), cat.clone(), sch.clone());
                let routines_l1 = routines_all.clone();
                self.l1_write(move |l1| {
                    l1.set_tables(&cid_c, &cat_c, Some(&sch_c), t);
                    l1.set_views(&cid_c, &cat_c, Some(&sch_c), v);
                    l1.set_sequences(&cid_c, &cat_c, Some(&sch_c), s);
                    l1.set_triggers(&cid_c, &cat_c, Some(&sch_c), g);
                });
                self.l1_write_routines(&cid, &cat, &sch, routines_l1);
            }
            // L2：全类别 + 索引重建 + 完成标记（半成品防御：先清残留再整块重写）
            if let Some(mut c) = self.cache(conn_id) {
                c.prune_schema(catalog, schema);
                let name = schema.to_string();
                c.put_schemas(catalog, std::slice::from_ref(&name));
                if let Some(sid) = c.schema_id(catalog, schema) {
                    c.put_objects(sid, &tables_half);
                    c.put_objects(sid, &views_half);
                    c.put_routines(sid, &routines_all);
                    c.put_sequences(sid, &objects.sequences);
                    c.put_triggers(sid, &objects.triggers);
                }
                c.rebuild_index(catalog, schema);
                c.mark_schema_complete(catalog, schema);
            }
            (tables_half, views_half, table_count)
        } else {
            // 对象完整：列段目标从 L2 读（表/视图名；列查询对两者一致）
            let nodes: Vec<NodeInfo> = l2_names
                .iter()
                .map(|n| NodeInfo::new(n.clone(), SchemaObjectKind::Table))
                .collect();
            (nodes, vec![], l2_names.len())
        };
        // 列段（独立判断，体验优于 DBeaver：DBeaver 展开表节点时才同步加载列；
        // 我们把「列」也纳入展开 schema 的后台预热 → 用户点表即 0ms 缓存命中）。
        // **全量**（2026-10-09 批量列接口）：小 schema 走默认实现循环（本地驱动
        // 无建连成本）；大 schema（如 public 24 表）走 PG/MySQL 批量覆盖——复用
        // 常驻连接 + 循环列 SQL，一次建连全部列（24 表 ≈ 500ms 后台，替代 24 次
        // 新建连接 7s+）。失败单表不阻断整批（点表时实时回源兜底）。
        if !columns_done {
            let column_targets: Vec<String> = tables_half
                .iter()
                .chain(views_half.iter())
                .map(|n| n.name.clone())
                .collect();
            if !column_targets.is_empty() {
                let pairs = match self
                    .metadata
                    .list_columns_batch(conn_id, catalog, schema, &column_targets)
                    .await
                {
                    Ok(p) => p,
                    Err(e) => {
                        tracing::warn!(conn_id, catalog, schema, err = %e, "批量列预热失败（点表时实时回源）");
                        vec![]
                    }
                };
                let mut warmed = 0usize;
                let cache = self.cache(conn_id);
                let schema_id = cache.as_ref().and_then(|c| c.schema_id(catalog, schema));
                for (tname, cols) in &pairs {
                    if let (Some(c), Some(sid)) = (&cache, schema_id) {
                        c.put_columns(sid, tname, cols);
                    }
                    let store = cols.clone();
                    self.l1_write(move |l1| {
                        l1.set_columns_detail(conn_id, catalog, Some(schema), tname, store)
                    });
                    warmed += 1;
                }
                tracing::info!(conn_id, catalog, schema, warmed, targets = column_targets.len(), "后台预热表列完成（点表 0ms）");
            }
        }
        tracing::info!(
            conn_id,
            catalog,
            schema,
            elapsed_ms = t.elapsed().as_millis(),
            tables = table_count,
            "后台预热 schema 完成（DBeaver loadChildren 对齐）"
        );
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

    /// 编辑器补全的实时内省回退（§710）：把整条连接的表 / 视图 / 列按 cache-aside
    /// 写进 L2。
    ///
    /// 复用导航的 [`Self::collect_objects`] / [`Self::load_columns`]（同一套
    /// L1→L2→实时内省→写回），不另起一套。大 schema 的列按分级门禁跳过
    /// （与 C2 邻接预取同一口径：`should_load_columns()` 为假时不逐表拉列，
    /// 列候选留给用户展开表时按需内省）——补全不该给源库加整库负载。
    ///
    /// 调用时机：补全端口首次未命中且 NavCache 无对象时，经导航队列提交
    /// （[`crate::nav_jobs::enqueue_warm_completion`]，与导航内省共享信号量 /
    /// 让位）；完成前补全只给关键字与函数（如实，不假装有元数据）。
    pub async fn warm_completion_metadata(&self, conn_id: &str) -> Result<usize, CoreError> {
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
        let mut touched = 0usize;
        for catalog in catalogs {
            let schemas = self
                .metadata
                .list_schemas(conn_id, &catalog)
                .await
                .unwrap_or_default();
            let schemas = if schemas.is_empty() {
                vec![FALLBACK_CONTAINER.to_string()]
            } else {
                schemas
            };
            for schema in schemas {
                // 表 + 视图：一次批量内省（collect_objects 内部按 L1→L2→实时→写回）
                let mut batch = None;
                let tables = self
                    .collect_objects(conn_id, &catalog, &schema, NavFolder::Tables, &mut batch)
                    .await?;
                let views = self
                    .collect_objects(conn_id, &catalog, &schema, NavFolder::Views, &mut batch)
                    .await?;
                touched += tables.len() + views.len();
                // 列：分级门禁（与 C2 预取同口径）——大库不为补全逐表拉列
                if engine::driver::get_level(conn_id).should_load_columns() {
                    for object in tables.iter().chain(views.iter()) {
                        self.load_columns(conn_id, &catalog, &schema, &object.name)
                            .await?;
                    }
                }
            }
        }
        Ok(touched)
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
            // 每表单独设 5s 超时：单张表慢/卡（如池连接被其他任务占用）只跳过该表，
            // 不再让整批预取被一次 30s 外层超时全部放弃（真机：min=1 池并发时整批 30s 超时，
            // 列缓存留半成品，下次展开表仍要回源）。
            match tokio::time::timeout(
                std::time::Duration::from_secs(5),
                self.load_columns(conn_id, catalog, schema, table),
            )
            .await
            {
                Ok(Ok(_)) => ok += 1,
                Ok(Err(e)) => tracing::debug!(
                    conn_id = %conn_id,
                    catalog = %catalog,
                    schema = %schema,
                    table = %table,
                    error = %e,
                    "列预取失败（不影响可见状态；日志级别调 DEBUG 可查）"
                ),
                Err(_) => tracing::warn!(
                    conn_id = %conn_id,
                    catalog = %catalog,
                    schema = %schema,
                    table = %table,
                    "列预取单表超时（跳过该表，其余继续；用户展开该表时按需回源）"
                ),
            }
        }
        ok
    }

    /// 加载对象属性（属性面板用；连接 / Catalog / Schema 无需查询）。
    ///
    /// `type_id` = 数据库类型 id（`drivers.type_id`，属性源分派键）；
    /// `driver` = 驱动 id / 显示名（fallback「驱动」行）。
    pub async fn load_properties(
        &self,
        ref_: &PropertyRef,
        conn_label: &str,
        driver: &str,
        type_id: Option<&str>,
        db_type: Option<&str>,
        data_req: Option<&crate::table_data::TableDataRequest>,
    ) -> Result<property::ObjectProperties, CoreError> {
        crate::property_panel::load_properties(
            &self.metadata,
            ref_,
            conn_label,
            driver,
            type_id,
            db_type,
            data_req,
        )
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

    /// S3-② 分区/继承折叠：有 `parent_name` 的表不单独产出节点，父表带计数徽标。
    ///
    /// 入参：`orders`（父表）+ 两个分区子表 + 一个继承子表 + `line_items`（父表）+ 一个分区子表。
    /// 期望：只产出 2 个父表节点；`orders` 计数 = 分区 2 / 继承 1；`line_items` = 分区 1 / 继承 0。
    #[test]
    fn partition_children_collapse_into_parent_with_counts() {
        let nodes = NavigatorService::object_nodes(
            "P_1",
            "main",
            "public",
            NavFolder::Tables,
            vec![
                NodeInfo::new("orders", SchemaObjectKind::Table),
                NodeInfo::new("orders_2024", SchemaObjectKind::Table)
                    .with_parent("orders")
                    .with_partition(),
                NodeInfo::new("orders_2025", SchemaObjectKind::Table)
                    .with_parent("orders")
                    .with_partition(),
                NodeInfo::new("orders_archive", SchemaObjectKind::Table)
                    .with_parent("orders"),
                NodeInfo::new("line_items", SchemaObjectKind::Table),
                NodeInfo::new("line_items_2024", SchemaObjectKind::Table)
                    .with_parent("line_items")
                    .with_partition(),
            ],
        );

        assert_eq!(nodes.len(), 2, "子表不单独列出，只留下父表");
        let orders = nodes.iter().find(|n| n.name == "orders").expect("父表 orders 存在");
        assert_eq!(orders.child_partitions, 2);
        assert_eq!(orders.child_inherited, 1);
        let line_items = nodes.iter().find(|n| n.name == "line_items").expect("父表 line_items 存在");
        assert_eq!(line_items.child_partitions, 1);
        assert_eq!(line_items.child_inherited, 0, "继承子表计数归零不缺失");
    }

    /// S3-②：普通表（无子表）计数两半都为零——行尾不显示徽标的前置条件。
    #[test]
    fn plain_table_has_zero_child_counts() {
        let nodes = NavigatorService::object_nodes(
            "P_1",
            "main",
            "public",
            NavFolder::Tables,
            vec![NodeInfo::new("t1", SchemaObjectKind::Table)],
        );
        assert_eq!(nodes[0].child_partitions, 0);
        assert_eq!(nodes[0].child_inherited, 0);
    }

    /// 大 schema 阈值（>50 列）：不逐列生成节点，改单个提示节点跳表属性面板。
    #[test]
    fn oversized_column_sets_collapse_into_one_overflow_node() {
        let cols: Vec<ColumnDetail> = (0..60)
            .map(|i| ColumnDetail {
                name: format!("c{i:02}"),
                data_type: "int".to_string(),
                nullable: true,
                is_primary_key: false,
                is_foreign_key: false,
                references: None,
                ordinal: 0,
                default_value: None,
                comment: None,
                is_identity: false,
                identity_generation: None,
                collation: None,
                extra: Default::default(),
            })
            .collect();
        let nodes = NavigatorService::column_nodes("G_1", "db", "public", "big_t", cols);
        assert_eq!(nodes.len(), 1, "超阈值只生成一个提示节点");
        assert_eq!(nodes[0].kind, NavNodeKind::ColumnOverflow);
        assert!(nodes[0].name.contains("60"));
        // 提示节点的属性定位指向表本身 → 点击打开表属性面板（列页签）。
        let prop = nodes[0].property.as_ref().expect("提示节点应带属性定位");
        assert_eq!(prop.kind, PropertyKind::Table);
        assert_eq!(prop.name, "big_t");
    }
}

#[cfg(test)]
mod warm_completion_tests {
    //! 【§710】补全实时内省回退的安全属性。
    //!
    //! 手法同 `l1_tests` / `paging_tests`：给**空的**连接管理器——任何走实时内省的
    //! 路径都必然失败（`CONN_NOT_FOUND`）。「补全内省对不存在连接一拍内失败」是它的
    //! 安全底线：编辑器补全未命中时在后台线程里调它，失败就该快速结束，
    //! 之后补全继续只给关键字与函数（如实，不假装有元数据）。
    //! 真实内省路径（活连接 → cache-aside 写 L2）需要真实端点，留给真机验证。

    use std::sync::Arc;

    use engine::connection_manager::ConnectionManager;

    use super::*;

    /// 空连接管理器下，`warm_completion_metadata` 必须快速失败（不 panic、不卡死）。
    #[test]
    fn warm_completion_metadata_fails_fast_without_a_live_connection() {
        let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
        let svc =
            NavigatorService::with_context(Arc::new(ConnectionManager::new()), None, false);
        let result = rt.block_on(svc.warm_completion_metadata("P_no_such_conn_710"));
        assert!(
            result.is_err(),
            "空管理器下实时内省必须失败（断言快速失败而非静默空成功）：{result:?}"
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
            is_identity: false,
            identity_generation: None,
            collation: None,
            extra: Default::default(),
            references: None,
            ordinal: 0,
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
            .load_children(conn_id, &NavPath::Connection, None)
            .await
            .expect("L1 命中应成功（无需真实连接）");
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].name, "main");

        // 反证：同一服务下，没有 L1 条目的连接必须失败（否则上面的断言无意义）
        assert!(svc
            .load_children("P_l1_test_no_such_conn", &NavPath::Connection, None)
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
                None,
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
                None,
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
            svc.load_children(conn_id, &NavPath::Connection, None)
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
        let _ = svc.load_children(conn_id, &NavPath::Connection, None).await;

        let cache = l1();
        let mut guard = cache.lock().expect("锁 L1");
        assert!(
            guard.get_catalogs(conn_id).is_none(),
            "刷新后该连接的 L1 条目应被清掉"
        );
    }

    /// 一个类别读不出来，不该把整个 schema 层拖成一行红字。
    ///
    /// 手法：只喂「表」这一类进 L1，其余类别必然走实时内省（空管理器下必失败）。
    /// 展开 schema 这一层若仍报错，用户就连看得见的表也拿不到了。
    /// 类别文件夹**固定显示**（用户决策 2026-10-08，对齐 DBeaver）：失败的类别
    /// 也显示（count=0），用户一眼看到"这个 schema 有哪些类别"。
    #[tokio::test]
    async fn one_failing_category_does_not_hide_the_rest() {
        let conn_id = "P_folders_partial_fail";
        {
            let cache = l1();
            let mut guard = cache.lock().expect("锁 L1");
            guard.invalidate_connection(conn_id);
            guard.set_tables(conn_id, "main", Some("public"), vec![table("t1")]);
        }

        // DBeaver 分块模型（2026-10-08）：展开 schema **不内省对象**，静态返回 5 个
        // 类别文件夹（无计数）——即使 L1 已有表数据也不预查（点击文件夹才加载对象）。
        let nodes = offline_service(false)
            .load_children(
                conn_id,
                &NavPath::Schema {
                    catalog: "main".to_string(),
                    schema: "public".to_string(),
                },
                None,
            )
            .await
            .expect("静态文件夹永不失败");
        assert_eq!(nodes.len(), NavFolder::ALL.len(), "五类文件夹固定显示");
        let tables = nodes
            .iter()
            .find(|n| matches!(n.kind, NavNodeKind::Folder(NavFolder::Tables)))
            .expect("表文件夹在");
        assert_eq!(tables.name, "表", "静态文件夹不带计数（点击后才知对象数）");
        assert!(
            nodes
                .iter()
                .any(|n| matches!(n.kind, NavNodeKind::Folder(NavFolder::Sequences))),
            "序列文件夹也固定显示（无论有无对象）"
        );
    }

    /// 反证：对象加载（点击文件夹）时回源失败必须如实报错，不能给空树假装「没有对象」。
    #[tokio::test]
    async fn all_categories_failing_is_still_an_error() {
        let conn_id = "P_folders_all_fail";
        // 静态文件夹层：永不失败（不查库）。
        offline_service(false)
            .load_children(
                conn_id,
                &NavPath::Schema {
                    catalog: "main".to_string(),
                    schema: "public".to_string(),
                },
                None,
            )
            .await
            .expect("静态文件夹不依赖库，恒成功");
        // 对象层（点击 Tables 文件夹）：管理器为空 → 回源失败 → 如实报错。
        let err = offline_service(false)
            .load_objects(conn_id, "main", "public", NavFolder::Tables)
            .await;
        assert!(err.is_err(), "对象加载失败仍是失败（否则用户会以为这个 schema 下什么都没有）");
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
                None,
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
            .load_children_page(conn_id, &path, offset, page_size, None)
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
            .load_children_page(conn_id, &folder("public", NavFolder::Tables), 0, PAGE, None)
            .await
            .expect("首屏");
        let second = svc
            .load_children_page(conn_id, &folder("public", NavFolder::Tables), PAGE, PAGE, None)
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
            .load_children_page(conn_id, &folder("public", NavFolder::Tables), 2 * PAGE, PAGE, None)
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
            .load_children_page(conn_id, &folder("public", NavFolder::Views), 0, PAGE, None)
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
            .load_children_page(conn_id, &folder("public", NavFolder::Tables), 0, PAGE, None)
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
                .load_children_page(conn_id, &folder("public", NavFolder::Tables), 0, PAGE, None)
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
            .load_children_page(conn_id, &folder("public", NavFolder::Tables), 0, PAGE, None)
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
            .load_children(conn_id, &folder("public", NavFolder::Routines), None)
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
            .load_children(conn_id, &folder("public", NavFolder::Sequences), None)
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
            .load_children(conn_id, &folder("public", NavFolder::Triggers), None)
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
            svc.load_children(conn_id, &folder("public", NavFolder::Sequences), None)
                .await
                .is_err(),
            "刷新模式应走实时内省（空连接管理器下必然失败）"
        );

        cleanup(&root, conn_id);
    }
}

#[cfg(test)]
mod system_catalog_filter_tests {
    //! S3-① 系统库 / 系统 schema 判定与可见性过滤（纯函数，无 I/O）。

    use std::collections::BTreeSet;

    use super::{nav_is_system_catalog, nav_is_system_schema, nav_keep_object};

    #[test]
    fn system_catalogs_are_detected_case_insensitively() {
        for sys in ["information_schema", "mysql", "performance_schema", "sys", "admin", "local"] {
            assert!(nav_is_system_catalog(sys), "小写命中: {sys}");
            assert!(
                nav_is_system_catalog(&sys.to_ascii_uppercase()),
                "大写命中（大小写不敏感）: {sys}"
            );
        }
        // main 是 SQLite/DuckDB 的单一库与 FALLBACK_CONTAINER，永不隐藏。
        assert!(!nav_is_system_catalog("main"));
        assert!(!nav_is_system_catalog("postgres"));
        assert!(!nav_is_system_catalog("analytics"));
    }

    #[test]
    fn system_schemas_are_detected_with_temp_prefixes() {
        for sys in ["pg_catalog", "pg_toast", "information_schema"] {
            assert!(nav_is_system_schema(sys), "命中: {sys}");
        }
        assert!(nav_is_system_schema("pg_temp_1"), "PG 临时 schema 前缀");
        assert!(nav_is_system_schema("pg_toast_temp_1234"), "PG toast 临时前缀");
        assert!(!nav_is_system_schema("public"));
        assert!(!nav_is_system_schema("main"));
        assert!(!nav_is_system_schema("app_public"));
    }

    #[test]
    fn keep_object_default_mode_hides_system_objects() {
        let none = None;
        // 默认模式（未设置白名单）：系统对象隐藏、用户对象保留。
        assert!(!nav_keep_object("mysql", none, nav_is_system_catalog));
        assert!(nav_keep_object("postgres", none, nav_is_system_catalog));
        assert!(!nav_keep_object("pg_catalog", none, nav_is_system_schema));
        assert!(nav_keep_object("public", none, nav_is_system_schema));
    }

    #[test]
    fn keep_object_whitelist_mode_shows_exactly_the_set() {
        let set = BTreeSet::from(["mysql".to_string(), "analytics".to_string()]);
        let visible = Some(&set);
        // 白名单模式：原样展示集合里的名字（含系统对象）。
        assert!(nav_keep_object("mysql", visible, nav_is_system_catalog));
        assert!(nav_keep_object("analytics", visible, nav_is_system_catalog));
        assert!(!nav_keep_object("postgres", visible, nav_is_system_catalog));
        // 空集合 = 显式全隐藏（弹框里全部取消勾选）。
        let empty = BTreeSet::new();
        assert!(!nav_keep_object("analytics", Some(&empty), nav_is_system_catalog));
    }
}

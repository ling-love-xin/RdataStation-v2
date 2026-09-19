//! 导航侧 L2 元数据缓存（cache-aside）。
//!
//! 定位：把 `NavigatorService` 的实时内省结果按连接落到 engine 的连接级 SQLite 缓存
//! （`MetadataCacheManager` 定位路径：全局连接 → 系统目录，项目 / 共享连接 → 项目目录），
//! 下次展开同一路径时优先命中缓存。
//!
//! 语义：
//! - **读**：命中即返回；未命中 / 读取出错一律返回 `None`，由调用方回落实时内省；
//! - **写**：`put_*` 尽力而为（失败静默），保证「内省结果永远优先于缓存」；
//! - **刷新**：`fresh` 模式下调用方跳过读缓存，并在重写前清理该 schema 的旧行，
//!   避免已删除对象残留在缓存里。
//!
//! 范围（C3 首版）：schema / 表 / 视图 / 列；存储过程、序列、触发器仍走实时内省。
//! 缓存**永不自动删除**（设计 §5.2），清理只经「缓存管理」对话框。

use engine::driver::traits::{ColumnDetail, NodeInfo, SchemaObjectKind};
use engine::persistence::{
    ChunkResult, ConnectionType, FtsSearchResult, IndexEntry, IndexSearchHit, MetadataCacheManager,
    MetadataCacheOps,
    MetadataCachePool, SchemaObjectCounts,
};

use crate::model::NavSource;

/// 该连接是否已有**落盘**缓存（只查路径，不建文件）。
///
/// 跨连接搜索前用它过滤：搜索扫的是各连接自己的 L2 索引，但**不能因为用户敲了个字
/// 就给从未内省过的连接建出缓存文件**（`NavCache::open` 会建库并跑迁移）。
pub fn cache_file_exists(conn_id: &str, project_root: Option<&str>) -> bool {
    let connection_type = match NavSource::from_conn_id(conn_id) {
        NavSource::Global => ConnectionType::Global,
        _ => ConnectionType::Project,
    };
    MetadataCacheManager::new(conn_id, connection_type, project_root)
        .map(|manager| manager.db_path().exists())
        .unwrap_or(false)
}

/// 缓存中表示「视图」的 `table_type`（与 [`NavCache::put_objects`] 的写入约定一致）。
const TABLE_TYPE_VIEW: &str = "VIEW";
/// 缓存中表示「表」的 `table_type`（受 `tables.table_type` CHECK 约束限制，取值见迁移 `004_refactor_to_normalized.sql`）。
const TABLE_TYPE_BASE: &str = "TABLE";

/// 连接级导航缓存句柄（持有一个缓存 SQLite 连接）。
pub struct NavCache {
    /// 归属连接（`metadata_index` 写入与 L1 回填需要）
    conn_id: String,
    ops: MetadataCacheOps,
}

impl NavCache {
    /// 打开连接级缓存。
    ///
    /// 路径不可用（如项目连接缺少项目根）或打开失败时返回 `None`，调用方降级为实时内省。
    pub fn open(conn_id: &str, project_root: Option<&str>) -> Option<Self> {
        let connection_type = match NavSource::from_conn_id(conn_id) {
            NavSource::Global => ConnectionType::Global,
            _ => ConnectionType::Project,
        };
        // 路径仍由 `MetadataCacheManager` 唯一定义（布局知识不在本 crate 重写）。
        let manager = MetadataCacheManager::new(conn_id, connection_type, project_root).ok()?;

        // 池化取用：把「开文件 + 5 条 PRAGMA + 一次迁移校验」从**每次缓存访问**
        // 挪到「每个缓存文件一次」。以前每次 `open()` 都要付这份固定开销，
        // 与「L2 命中 <5ms」的设计目标相抵（L1 空时尤其明显）。
        let pool = MetadataCachePool::get_or_create(manager.db_path().clone(), 2).ok()?;
        let guard = pool.acquire().ok()?;

        Some(Self {
            conn_id: conn_id.to_string(),
            ops: MetadataCacheOps::from_pooled(guard),
        })
    }

    // ==================== schema ====================

    /// 读取某 catalog 下的 schema 名（空结果视为未命中）。
    pub fn schemas(&self, catalog: &str) -> Option<Vec<String>> {
        let rows = self.ops.list_schemas(Some(catalog)).ok()?;
        if rows.is_empty() {
            return None;
        }
        Some(rows.into_iter().map(|s| s.schema_name).collect())
    }

    /// 读**全部** schema（含 catalog 与 `schema_id`）——给“一次把候选目录装进内存”的调用方用
    ///
    /// 与 [`Self::schemas`] 的区别：不按 catalog 过滤、带 `id`（后续 `objects` / `columns` 都要它），
    /// 且**空结果就是空**（不是“未命中”）——调用方是自己决定要不要回落实时内省的。
    /// 目前唯一使用方是 SQL 补全的候选预载（`editor/src/completion.rs` 的对端）。
    pub fn all_schemas(&self) -> Vec<(String, String, i64)> {
        match self.ops.list_schemas(None) {
            Ok(rows) => rows
                .into_iter()
                // catalog 名在缓存里可空（部分驱动不报）——候选用不上它，空串即可
                .map(|s| (s.catalog_name.unwrap_or_default(), s.schema_name, s.id))
                .collect(),
            Err(e) => {
                tracing::warn!(connection_id = %self.conn_id, error = %e, "读取全部 schema 失败（候选目录本次为空）");
                Vec::new()
            }
        }
    }

    /// 回写 schema 名（须先于对象写入，后续按名取 `schema_id`）。
    pub fn put_schemas(&self, catalog: &str, names: &[String]) {
        for name in names {
            if let Err(e) = self.ops.save_schema(catalog, name, None, None) {
                tracing::warn!(schema = %name, error = %e, "schema 写入缓存失败（下次展开会回源库）");
            }
        }
    }

    /// 按名取 `schema_id`。
    pub fn schema_id(&self, catalog: &str, schema: &str) -> Option<i64> {
        self.ops.get_schema_id(catalog, schema).ok().flatten()
    }

    /// 重建该 schema 的 `metadata_index`（大 schema 分页 / 计数 / 搜索的数据源）。
    ///
    /// 只在**冷启动内省写入之后**调用（命中路径只读不写，不触发重建）。
    /// 尽力而为：索引是加速设施，失败只告警——下次冷启动会重建。
    pub fn rebuild_index(&mut self, catalog: &str, schema: &str) {
        if let Err(e) = self
            .ops
            .rebuild_schema_index(&self.conn_id, catalog, schema)
        {
            tracing::warn!(
                catalog,
                schema,
                error = %e,
                "重建元数据索引失败（分页 / 计数退化为实时内省）"
            );
        }
    }

    /// 清理某 schema 的缓存行（刷新时先删后写；依赖外键级联删除表 / 列）。
    pub fn prune_schema(&mut self, catalog: &str, schema: &str) {
        if let Some(id) = self.schema_id(catalog, schema) {
            // 失败不能静默：刷新路径全靠它清掉旧行。历史上底层 `delete_schema` 引用了
            // 不存在的 `views` 表（整事务回滚），而这里 `let _ =` 把现象吞成了
            // 「点了刷新但旧对象还在」——排查成本极高，故改为告警留痕。
            if let Err(e) = self.ops.delete_schema(id) {
                tracing::warn!(
                    catalog,
                    schema,
                    error = %e,
                    "清理导航缓存失败：已删除的对象会残留在缓存里"
                );
            }
        }
    }

    // ==================== 表 / 视图 ====================

    /// 读取某 schema 下的对象（`want_view` 区分表 / 视图；空结果视为未命中）。
    pub fn objects(
        &self,
        schema_id: i64,
        want_view: bool,
    ) -> Option<Vec<(String, Option<String>)>> {
        let rows = self.ops.list_tables_normalized(schema_id, None).ok()?;
        let out: Vec<(String, Option<String>)> = rows
            .into_iter()
            .filter(|t| (t.table_type == TABLE_TYPE_VIEW) == want_view)
            .map(|t| (t.table_name, t.table_comment))
            .collect();
        if out.is_empty() {
            None
        } else {
            Some(out)
        }
    }

    /// 该 schema 在 `metadata_index` 里的计数（分块读的前置判断）。
    ///
    /// 索引为空（冷启动、尚未重建）时返回 `Some(全 0)`——调用方**不能**把它当作
    /// 「schema 里没有对象」，而要回落实时内省；故这里把「表/视图计数均为 0」视为不可用。
    pub fn object_counts(&self, schema_id: i64) -> Option<SchemaObjectCounts> {
        let counts = self
            .ops
            .get_schema_object_counts(&self.conn_id, schema_id)
            .ok()?;
        if counts.table_count == 0 && counts.view_count == 0 && counts.routine_count == 0 {
            return None;
        }
        Some(counts)
    }

    /// 分块读取某 schema 下的表 / 视图名（`want_view` 区分类别）。
    ///
    /// 返回 `None` 表示「索引里没有这一类别」——两种情形：索引尚未重建（冷启动），
    /// 或该类别真的为空。两者都应由调用方回落实时内省，以实时结果为准。
    pub fn objects_chunk(
        &self,
        schema_id: i64,
        want_view: bool,
        offset: usize,
        limit: usize,
    ) -> Option<ChunkResult<IndexEntry>> {
        let object_type = if want_view { "view" } else { "table" };
        let chunk = self
            .ops
            .get_objects_chunk(
                &self.conn_id,
                Some(schema_id),
                object_type,
                offset as i64,
                limit as i64,
            )
            .ok()?;
        if chunk.total == 0 {
            return None;
        }
        Some(chunk)
    }

    /// 目标对象在某类别里的**零基位次**（与 [`Self::objects_chunk`] 同一排序口径）。
    ///
    /// 用途：搜索命中「在树中定位」时，大 schema 只加载了首屏，需要直接跳到目标所在的那一页。
    ///
    /// 返回 `None` 的两种情形必须区分对待（由调用方决定文案）：
    /// 索引里**没有这个对象**（未重建 / 已删），或索引读取失败。
    /// 两者都不能当作「位次 0」——那会把用户送到第一页并声称定位成功。
    pub fn object_position(&self, schema_id: i64, want_view: bool, name: &str) -> Option<usize> {
        let object_type = if want_view { "view" } else { "table" };
        match self
            .ops
            .get_object_position(&self.conn_id, Some(schema_id), object_type, name)
        {
            Ok(pos) => pos.map(|p| p.max(0) as usize),
            Err(e) => {
                tracing::warn!(
                    connection_id = %self.conn_id,
                    schema_id,
                    object_type,
                    name,
                    error = %e,
                    "对象位次查询失败（本次不做定位）"
                );
                None
            }
        }
    }

    /// 按名称搜索该连接的索引（跨 schema 中缀匹配）。
    ///
    /// 失败 / 无命中都返回空表：搜索是尽力而为的交互操作，不该因为某条连接缓存损坏
    /// 就整体报错（但要告警留痕，否则“搜不到”会被当成“库里没有”）。
    pub fn search_index(&self, needle: &str, limit: usize) -> Vec<IndexSearchHit> {
        match self.ops.search_index(&self.conn_id, needle, limit as i64) {
            Ok(hits) => hits,
            Err(e) => {
                tracing::warn!(
                    connection_id = %self.conn_id,
                    needle,
                    error = %e,
                    "元数据索引搜索失败（本次无结果）"
                );
                Vec::new()
            }
        }
    }

    /// 按内容搜索该连接的 FTS 索引（注释 / 数据类型；跨 schema 子串匹配）。
    ///
    /// 与名称档同一口径：失败 / 无命中都返回空表（搜索是尽力而为的交互操作），
    /// 但告警留痕，否则「搜不到」会被当成「库里没有」。
    ///
    /// 门槛：trigram 分词器下**少于 3 个字符必然无命中**（不足一个 trigram）——
    /// 调用方应先拦住过短的词（Quick Open 的 `#` 档按 3 字提示）。
    pub fn search_fts(&self, needle: &str, limit: usize) -> Vec<FtsSearchResult> {
        match self.ops.search_fts(needle, None) {
            Ok(hits) => hits.into_iter().take(limit).collect(),
            Err(e) => {
                tracing::warn!(
                    connection_id = %self.conn_id,
                    needle,
                    error = %e,
                    "元数据全文搜索失败（本次无结果）"
                );
                Vec::new()
            }
        }
    }

    /// 回写表 / 视图对象。
    pub fn put_objects(&self, schema_id: i64, objects: &[NodeInfo]) {
        for obj in objects {
            let is_view = obj.kind == SchemaObjectKind::View;
            let table_type = if is_view {
                TABLE_TYPE_VIEW
            } else {
                TABLE_TYPE_BASE
            };
            match self.ops.save_table(
                schema_id,
                &obj.name,
                table_type,
                obj.comment.as_deref(),
                None,
                None,
            ) {
                Ok(table_id) => {
                    if is_view {
                        if let Err(e) = self.ops.save_view(table_id, "", None, None) {
                            tracing::warn!(
                                object = %obj.name,
                                error = %e,
                                "视图定义写入缓存失败（下次展开会回源库）"
                            );
                        }
                    }
                }
                Err(e) => tracing::warn!(
                    object = %obj.name,
                    error = %e,
                    "对象写入缓存失败（下次展开会回源库）"
                ),
            }
        }
    }

    // ==================== 列 ====================

    /// 读取某表的列（空结果视为未命中）。
    pub fn columns(&self, schema_id: i64, table: &str) -> Option<Vec<ColumnDetail>> {
        let table_id = self.ops.get_table_id(schema_id, table).ok().flatten()?;
        let rows = self.ops.list_columns_normalized(table_id).ok()?;
        if rows.is_empty() {
            return None;
        }
        Some(
            rows.into_iter()
                .map(|c| ColumnDetail {
                    name: c.column_name,
                    data_type: c.data_type,
                    nullable: c.is_nullable,
                    is_primary_key: c.is_primary_key,
                    is_foreign_key: c.is_foreign_key,
                    default_value: c.column_default,
                    comment: c.column_comment,
                    extra: c.extra,
                })
                .collect(),
        )
    }

    /// 回写某表的列。
    pub fn put_columns(&self, schema_id: i64, table: &str, columns: &[ColumnDetail]) {
        let Some(table_id) = self.ops.get_table_id(schema_id, table).ok().flatten() else {
            return;
        };
        for (ix, col) in columns.iter().enumerate() {
            // 第 7 个参数是 `is_identity`（自增标识），不是 `is_unique`：
            // `columns` 表没有唯一性列（唯一性由 `indexes` 表达），而 `ColumnDetail`
            // 也不携带自增信息，所以保守写 false——主键走 `is_primary`，两者互不佑替。
            if let Err(e) = self.ops.save_column(
                table_id,
                &col.name,
                &col.data_type,
                ix as i32,
                col.nullable,
                col.is_primary_key,
                false,
                col.default_value.as_deref(),
                col.comment.as_deref(),
            ) {
                tracing::warn!(
                    table,
                    column = %col.name,
                    error = %e,
                    "列写入缓存失败（下次展开会回源库）"
                );
            }
        }
    }

    // ==================== 例程 / 序列 / 触发器 ====================
    //
    // 写侧一律**尽力而为**（写不进去下次仍会回源库，不影响正确性）——与
    // `put_objects` / `put_columns` 同一口径：缓存是加速设施，失败不阻断导航。
    //
    // 索引 / 约束**有意不在这里**：它们的消费方是属性面板，而属性面板按设计走
    // 实时内省（见 `property_panel.rs` 模块文档）——给实时语义套一层缓存是错的。

    /// 读取某 schema 的例程（存储过程 + 函数，空结果视为未命中）。
    pub fn routines(&self, schema_id: i64) -> Option<Vec<NodeInfo>> {
        let rows = self.ops.list_routines(schema_id, None).ok()?;
        if rows.is_empty() {
            return None;
        }
        Some(
            rows.into_iter()
                .map(|r| {
                    NodeInfo::new(r.routine_name, routine_kind(&r.routine_type))
                        .with_comment(r.routine_comment)
                })
                .collect(),
        )
    }

    /// 回写某 schema 的例程（只登记名字 / 类型 / 注释）。
    ///
    /// `save_routine` 是 `INSERT OR REPLACE`，而这里不给 `routine_definition`——
    /// 会把同名例程的定义列置空。当前**没有任何路径往 L2 写定义**（例程源码走
    /// `get_routine_source` 实时查询），所以无损；将来若要缓存定义，这里必须改成
    /// 「先查后增量更新」，否则展开一次例程文件夹就会把定义抹掉。
    pub fn put_routines(&self, schema_id: i64, routines: &[NodeInfo]) {
        for r in routines {
            if let Err(e) = self.ops.save_routine(
                schema_id,
                &r.name,
                routine_type_str(&r.kind),
                None,
                None,
                None,
                None,
                r.comment.as_deref(),
            ) {
                tracing::warn!(
                    routine = %r.name,
                    error = %e,
                    "例程写入缓存失败（下次展开会回源库）"
                );
            }
        }
    }

    /// 读取某 schema 的序列名（空结果视为未命中）。
    pub fn sequences(&self, schema_id: i64) -> Option<Vec<NodeInfo>> {
        let names = self.ops.list_sequences(schema_id).ok()?;
        if names.is_empty() {
            return None;
        }
        Some(
            names
                .into_iter()
                .map(|n| NodeInfo::new(n, SchemaObjectKind::Sequence))
                .collect(),
        )
    }

    /// 回写某 schema 的序列（只登记名字）。
    pub fn put_sequences(&self, schema_id: i64, sequences: &[NodeInfo]) {
        for s in sequences {
            if let Err(e) = self.ops.save_sequence_name(schema_id, &s.name) {
                tracing::warn!(sequence = %s.name, error = %e, "序列写入缓存失败（下次展开会回源库）");
            }
        }
    }

    /// 读取某 schema 的触发器（含所属表——回到 `NodeInfo::parent_name`）。
    pub fn triggers(&self, schema_id: i64) -> Option<Vec<NodeInfo>> {
        let rows = self.ops.list_triggers(schema_id).ok()?;
        if rows.is_empty() {
            return None;
        }
        Some(
            rows.into_iter()
                .map(|t| {
                    NodeInfo::new(t.name, SchemaObjectKind::Trigger)
                        .with_comment(t.comment)
                        .with_parent(t.table_name)
                })
                .collect(),
        )
    }

    /// 回写某 schema 的触发器。
    ///
    /// **没有所属表的触发器不写**：`triggers.table_id` 是 `NOT NULL`，而所属表是
    /// 驱动内省给的（`NodeInfo::parent_name`）——MySQL 那类不提供它的驱动就停在
    /// 「不缓存」（如实降级：下次仍回源库，不是丢数据）。
    pub fn put_triggers(&self, schema_id: i64, triggers: &[NodeInfo]) {
        for t in triggers {
            let Some(table) = t.parent_name.as_deref().filter(|p| !p.is_empty()) else {
                continue;
            };
            if let Err(e) = self.ops.save_trigger_for_table(schema_id, table, &t.name) {
                tracing::warn!(
                    trigger = %t.name,
                    table,
                    error = %e,
                    "触发器写入缓存失败（下次展开会回源库）"
                );
            }
        }
    }
}

/// `routines.routine_type` → 导航类别（与 [`routine_type_str`] 互逆）。
fn routine_kind(routine_type: &str) -> SchemaObjectKind {
    match routine_type.to_ascii_uppercase().as_str() {
        "FUNCTION" => SchemaObjectKind::Function,
        _ => SchemaObjectKind::Procedure,
    }
}

/// 导航类别 → `routines.routine_type`。
///
/// 取引用而不是取值：`SchemaObjectKind` 带数据（`Copy` 不成立），
/// 而调用点只是从 `&NodeInfo` 上读一个字段。
fn routine_type_str(kind: &SchemaObjectKind) -> &'static str {
    match kind {
        SchemaObjectKind::Function => "FUNCTION",
        _ => "PROCEDURE",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("rds_navcache_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("mkdir");
        dir
    }

    fn col(name: &str, primary: bool) -> ColumnDetail {
        ColumnDetail {
            name: name.to_string(),
            data_type: "INTEGER".to_string(),
            nullable: !primary,
            is_primary_key: primary,
            is_foreign_key: false,
            default_value: None,
            comment: None,
            extra: Default::default(),
        }
    }

    /// 搜索：`NavCache::search_index` 能搜到已重建索引的表 / 列（带 schema、所属表）。
    ///
    /// 顺便钉住“搜索不建文件”：无缓存的连接走 `cache_file_exists` 就应被跳过。
    #[test]
    fn search_index_reports_hits_and_missing_cache() {
        let root = temp_root("search");
        let root_s = root.to_string_lossy().to_string();
        let conn_id = "P_conn_search";

        // 尚未内省过的连接：不得因为有搜索就给它建出缓存文件
        assert!(
            !cache_file_exists(conn_id, Some(&root_s)),
            "无缓存时 cache_file_exists 必须为 false（搜索不应建文件）"
        );

        let mut cache = NavCache::open(conn_id, Some(&root_s)).expect("打开缓存");
        cache.put_schemas("main", &["public".to_string()]);
        let sid = cache.schema_id("main", "public").expect("schema_id");
        cache.put_objects(
            sid,
            &[NodeInfo::new("order_items", SchemaObjectKind::Table)],
        );
        cache.put_columns(sid, "order_items", &[col("order_id", true)]);
        cache.rebuild_index("main", "public");

        assert!(
            cache_file_exists(conn_id, Some(&root_s)),
            "内省并落盘后应能看到缓存文件"
        );

        let hits = cache.search_index("order", 50);
        let names: Vec<&str> = hits.iter().map(|h| h.object_name.as_str()).collect();
        assert!(names.contains(&"order_items"), "表应命中（得到：{names:?}）");
        assert!(names.contains(&"order_id"), "列也应命中（得到：{names:?}）");

        let table = hits
            .iter()
            .find(|h| h.object_name == "order_items")
            .expect("表命中");
        assert_eq!(table.schema_name.as_deref(), Some("public"));
        assert_eq!(table.catalog_name.as_deref(), Some("main"));
        let column = hits
            .iter()
            .find(|h| h.object_name == "order_id")
            .expect("列命中");
        assert_eq!(column.parent_name.as_deref(), Some("order_items"));

        // 无命中即空表（调用方据此显示“无命中”，不报错）
        assert!(cache.search_index("zzz_不存在", 50).is_empty());

        drop(cache);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 项目连接：写入 schema / 表 / 列，重开后（新连接）仍可命中（已落盘）。
    #[test]
    fn roundtrip_schemas_tables_columns() {
        let root = temp_root("roundtrip");
        let root_s = root.to_string_lossy().to_string();

        let cache = NavCache::open("P_conn_test", Some(&root_s)).expect("打开缓存");
        cache.put_schemas("main", &["public".to_string()]);
        assert_eq!(cache.schemas("main"), Some(vec!["public".to_string()]));
        let sid = cache.schema_id("main", "public").expect("schema_id");

        cache.put_objects(
            sid,
            &[NodeInfo::new("t1", SchemaObjectKind::Table)
                .with_comment(Some("表注释".to_string()))],
        );
        assert_eq!(
            cache.objects(sid, false),
            Some(vec![("t1".to_string(), Some("表注释".to_string()))])
        );
        // 视图维度为空 → 视为未命中（由调用方回落实时内省）。
        assert!(cache.objects(sid, true).is_none());

        cache.put_columns(sid, "t1", &[col("id", true), col("v", false)]);
        let got = cache.columns(sid, "t1").expect("列应命中");
        assert_eq!(got.len(), 2);
        assert!(got.iter().any(|c| c.name == "id" && c.is_primary_key));

        // 新连接（已落盘）仍命中。
        let reopened = NavCache::open("P_conn_test", Some(&root_s)).expect("重开缓存");
        let sid2 = reopened.schema_id("main", "public").expect("schema_id 2");
        assert!(reopened.columns(sid2, "t1").is_some());

        let _ = std::fs::remove_dir_all(&root);
    }

    /// 缺少项目根时，项目连接不可用缓存（降级实时内省，不建文件）。
    #[test]
    fn project_connection_without_root_has_no_cache() {
        assert!(NavCache::open("P_conn_no_root", None).is_none());
    }

    /// 刷新路径：`prune_schema` 必须真正清掉旧行。
    ///
    /// 这是「右键 ▸ 刷新元数据」的用户可见行为回归点——此前底层 `delete_schema`
    /// 引用了不存在的 `views` 表，整条事务回滚，刷新后旧对象仍命中缓存。
    #[test]
    fn prune_schema_clears_stale_rows() {
        let root = temp_root("prune");
        let root_s = root.to_string_lossy().to_string();

        let mut cache = NavCache::open("P_conn_prune", Some(&root_s)).expect("打开缓存");
        cache.put_schemas("main", &["public".to_string()]);
        let sid = cache.schema_id("main", "public").expect("schema_id");
        cache.put_objects(sid, &[NodeInfo::new("t_old", SchemaObjectKind::Table)]);
        cache.put_columns(sid, "t_old", &[col("id", true)]);
        assert!(cache.objects(sid, false).is_some(), "前置：应命中表");
        assert!(cache.columns(sid, "t_old").is_some(), "前置：应命中列");

        cache.prune_schema("main", "public");

        assert!(
            cache.schema_id("main", "public").is_none(),
            "schema 行应被清掉"
        );
        assert!(cache.objects(sid, false).is_none(), "刷新后不应命中旧表");
        assert!(cache.columns(sid, "t_old").is_none(), "刷新后不应命中旧列");

        let _ = std::fs::remove_dir_all(&root);
    }
}

#[cfg(test)]
mod pool_wiring_tests {
    //! `NavCache` 与连接池的接线证明。
    //!
    //! 手法：先自己占住池里的一个连接，再开 `NavCache` —— 若它确实从**同一个池**取连接，
    //! 池的空闲数会降到 0。这样能区分「接了池」与「自己另开了一个连接/另一个池」。

    use super::*;

    /// 与 `NavCache::open` 同源的缓存文件路径（走 `MetadataCacheManager`，不硬编码布局）。
    fn cache_db_path(conn_id: &str, root: &str) -> std::path::PathBuf {
        MetadataCacheManager::new(conn_id, ConnectionType::Project, Some(root))
            .expect("构造缓存管理器")
            .db_path()
            .clone()
    }

    #[test]
    fn nav_cache_borrows_from_the_shared_pool() {
        let root = std::env::temp_dir().join(format!("rds_navpool_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("建目录");
        let root_s = root.to_string_lossy().to_string();
        let conn_id = "P_conn_pool_wiring";

        let db_path = cache_db_path(conn_id, &root_s);
        let pool = MetadataCachePool::get_or_create(db_path, 2).expect("建池");
        assert_eq!(pool.idle_count(), 2, "预热 2 个连接");

        // 占住一个：池里只剩 1
        let held = pool.acquire().expect("占住连接");
        assert_eq!(pool.idle_count(), 1);

        // NavCache 打开后应把最后一个也取走 —— 证明它走的是这个池
        let cache = NavCache::open(conn_id, Some(&root_s)).expect("打开缓存");
        assert_eq!(
            pool.idle_count(),
            0,
            "NavCache 应从同一池取走连接（而不是自开文件或另建池）"
        );

        drop(held);
        drop(cache);
        assert_eq!(pool.idle_count(), 2, "两者释放后连接都应归还");

        let _ = std::fs::remove_dir_all(&root);
    }
}

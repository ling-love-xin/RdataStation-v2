//! 补全端口的工作台实现（B9）：把连接级元数据缓存变成编辑器的候选目录
//!
//! ## 数据从哪来
//!
//! `database::cache::NavCache`（导航侧 L2 缓存：schema / 表 / 视图 / 列）——**不是**实时内省：
//! 补全要的是“快 + 稳”，实时内省（`MetadataService`，async + 要活连接）留给导航面板。
//! 缓存里没有的连接就**没候选**（不猜、也不为了补全去建缓存库）。
//!
//! ## 为什么是「预载 + 内存读」
//!
//! 端口每次按键都会被问一次（`CompletionProvider::is_completion_trigger` 之后）。
//! **按键里做 I/O 是不能接受的**，所以这里：
//!
//! 1. 命中内存缓存 → 直接给；
//! 2. 没命中 → **起一次后台线程**把该连接的表 / 列装进内存（去重：一条连接同时只载一次），
//!    本次先给空目录；
//! 3. 载好之后下一次按键就有候选了（**不通知、不弹窗**：补全晚一拍是常态，不该打扰）。
//!
//! ## 通道决定限定名怎么写（**给错候选比不给更糟**）
//!
//! | 通道 | 候选形如 | 依据 |
//! | --- | --- | --- |
//! | 源库 | `schema.表` | 直连源库 |
//! | 本地加速 | `schema.表` | `USE rds_src` 之后不写限定名也好用（架构 D12 的实测结论） |
//! | 联邦 | `别名.schema.表`（L2 源是 **`别名.表`**，两段） | 联邦档的写作规范（原型 §4） |
//!
//! 联邦档还要读会话快照：只有**挂上了**的源才给候选——挂不上的源给出来只会让人写出跑不通的 SQL。
//!
//! ## 【#53】L1 新鲜度（TTL + generation）
//!
//! L1 三份数据（`loaded` / `keys` / `meta`）同源同代（同一遍 `load_raw_catalog` 产出），
//! 新鲜度只记一份 `cache_meta`：
//!
//! - **TTL**：命中后检查条目年龄 > [`COMPLETION_CACHE_TTL_SECS`]（5 分钟，对齐 engine
//!   `MetadataCache` 默认）→ 视为未命中，安排后台重载，本次给空。语义同 blanco `cache.rs`：
//!   **过期 = 未命中，宁缺毋滥**——不为了“看起来还有”给陈旧候选（用户 DDL 建了新表，
//!   最多 5 分钟补全就能跟上）。当前 `loading` 已挡「同连接并发载入」。
//! - **generation**：每次载入代际 +1；写回前发现已有**更高代**（未来若放开并发载入时，
//!   先开始的载入晚回来）→ 丢弃本次结果，防止旧结果覆盖新结果。

use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use editor::channel::ExecChannel;
use editor::completion::{
    filter_catalog_by_schema, Candidate, CandidateKind, Catalog, ColumnAnnotation, CompletionPort,
    TemplateSnippet,
};

use database::cache::NavCache;
use engine::duckdb::federation::{registry::MountedSource, session as fed_session};
use engine::sql::{ProjectionSource, SqlDialect};

use crate::panels::Shared;

/// 单条连接最多装多少表 / 视图（超了标记 `truncated`，不再往下装）
const MAX_OBJECTS: usize = 3_000;
/// 单条连接最多装多少列
const MAX_COLUMNS: usize = 30_000;

/// 【#53】L1 补全缓存 TTL（秒）：对齐 engine `MetadataCache` 默认 5 分钟。
///
/// 语义：过期 = 未命中（blanco `cache.rs` 同款）——本次返回空并安排后台重载，
/// 下一次按键就有候选；宁可这次不补，也不在按键路径上做 I/O、更不给陈旧候选。
/// 效果：用户 DDL 建表 / 改表后，补全最多 5 分钟跟上（此前 L1 载入后**永不刷新**）。
const COMPLETION_CACHE_TTL_SECS: i64 = 300;

/// 【#53】一条 L1 补全缓存条目的新鲜度元信息（`loaded` / `keys` / `meta` 三份同源同代，只记一份）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CacheEntryMeta {
    /// 载入代际：每次 `schedule_load` +1；写回时发现已有更高代（期间被重载过）→ 丢弃本次
    generation: u64,
    /// 载入完成时间（Unix 秒，`SystemTime` 基准）
    loaded_at_secs: i64,
}

impl CacheEntryMeta {
    /// 条目是否新鲜（未超 TTL）。`now_secs` 由调用方注入，便于测试钉边界。
    fn fresh(&self, now_secs: i64) -> bool {
        now_secs.saturating_sub(self.loaded_at_secs) <= COMPLETION_CACHE_TTL_SECS
    }
}

/// 工作台实现：候选目录（内存读；首次未命中时后台预载）
pub struct WorkbenchCompletion {
    shared: Shared,
    /// 已载好的原始目录（按 conn_id）：**未做通道限定**，限定在返回时按通道拼
    loaded: Arc<Mutex<HashMap<String, Catalog>>>,
    /// 【T2a-2】已载好的主键映射（按 conn_id → `schema.表` → 主键列；与 `loaded` 同一次预载）
    ///
    /// 为什么单独存一份而不是塞进 `Catalog`：补全候选只要「列名 + 类型」，主键是**写回**才
    /// 问的（`CompletionPort::primary_key`），两者读的时机不同；但数据来自同一份导航缓存，
    /// 所以**同一线程同一遍遍历**一起装（零额外 I/O），只是存两处、各取所需。
    keys: Arc<Mutex<HashMap<String, HashMap<String, Vec<String>>>>>,
    /// 正在载的 conn_id（去重：同一连接同时只载一次）
    loading: Arc<Mutex<HashSet<String>>>,
    /// 【P1】已载好的列元数据（按 conn_id → `schema.表` → 列元数据；与 `loaded` / `keys` 同一次预载）
    ///
    /// 为什么单独存一份：字段注释（`CompletionPort::column_annotations`）按「来源表 + 来源列」
    /// 查列注释 / 可空 / 默认，候选目录只有列名 + 类型，装不下这些位；数据来自同一份导航缓存，
    /// 所以**同一线程同一遍遍历**一起装（零额外 I/O），只是存三处、各取所需。
    meta: Arc<Mutex<HashMap<String, HashMap<String, Vec<ColumnMeta>>>>>,
    /// 【#53】L1 条目的新鲜度元信息（按 conn_id；`loaded` / `keys` / `meta` 三份共用一份）
    ///
    /// 载入完成时写入（generation + 时间戳）；`raw` / `keys_map` / `meta_map` 命中后
    /// 先查这里——超 TTL 视为未命中，安排重载（见 [`CacheEntryMeta::fresh`]）。
    cache_meta: Arc<Mutex<HashMap<String, CacheEntryMeta>>>,
    /// 【#53】载入代际计数（`cache_meta` 里 generation 的来源）
    ///
    /// 防御方向：当前 `loading` 已挡「同连接并发载入」，此计数为「未来放开并发载入」
    /// 预留的旧覆盖新防线——写回前比对代际，已有更高代则丢弃本次结果。
    generation: Arc<AtomicU64>,
    /// 【§710】已提交过「补全实时内省」的 conn_id（等 NavCache 被导航队列填上）。
    ///
    /// 与 `loading` 的差别：`loading` 挡**并发**（载完即移除）；这个挡**串行重试**
    /// （NavCache 空时每次未命中都会读空，不能每敲一个字就向导航队列投一次内省）。
    /// 命中对象后移除——连接元数据变化（如用户新建表后重新内省）时允许再投。
    warm_submitted: Arc<Mutex<HashSet<String>>>,
    /// 【B9 切片二】模板片段（启动时一次性预载；内存读——菜单弹出时会问它）
    templates: Arc<Mutex<Vec<TemplateSnippet>>>,
}

impl WorkbenchCompletion {
    pub fn new(shared: Shared) -> Self {
        Self {
            shared,
            loaded: Arc::new(Mutex::new(HashMap::new())),
            keys: Arc::new(Mutex::new(HashMap::new())),
            loading: Arc::new(Mutex::new(HashSet::new())),
            warm_submitted: Arc::new(Mutex::new(HashSet::new())),
            meta: Arc::new(Mutex::new(HashMap::new())),
            cache_meta: Arc::new(Mutex::new(HashMap::new())),
            generation: Arc::new(AtomicU64::new(0)),
            templates: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// 模板的**一次性预载**（启动时调；读完就不再碰库）
    ///
    /// 为什么不懒加载：模板菜单弹出时读它，而菜单不能在渲染路径上做 I/O。一次读完最省事。
    /// 读不到（全局库未就绪）就空着——菜单不摆那一项，不是报错。
    fn schedule_template_load(&self) {
        let templates = self.templates.clone();
        std::thread::spawn(move || {
            let loaded = load_templates();
            if let Ok(mut slot) = templates.lock() {
                *slot = loaded;
            }
        });
    }

    /// 原始目录（命中且新鲜即给；未命中 / 超 TTL 就安排一次后台预载，本次给空）
    fn raw(&self, conn_id: &str) -> Option<Catalog> {
        if let Ok(loaded) = self.loaded.lock()
            && let Some(catalog) = loaded.get(conn_id)
            && self.entry_fresh(conn_id)
        {
            return Some(catalog.clone());
        }
        self.schedule_load(conn_id);
        None
    }

    /// 【T2a-2】主键映射（命中且新鲜即给；未命中就安排一次后台预载，本次给空）
    ///
    /// 与 [`Self::raw`] 同一策略：预载是**一次性的**（`loaded` / `keys` 同线程同遍历），
    /// 这里未命中时 `schedule_load` 已在上一次 `raw` 命中检查里去重——重复调用是 no-op。
    fn keys_map(&self, conn_id: &str) -> Option<HashMap<String, Vec<String>>> {
        if let Ok(keys) = self.keys.lock()
            && let Some(map) = keys.get(conn_id)
            && self.entry_fresh(conn_id)
        {
            return Some(map.clone());
        }
        self.schedule_load(conn_id);
        None
    }

    /// 【P1】列元数据映射（命中且新鲜即给；未命中就安排一次后台预载，本次给空）
    ///
    /// 与 [`Self::keys_map`] 同一策略：未命中时 `schedule_load` 已去重，重复调用是 no-op。
    fn meta_map(&self, conn_id: &str) -> Option<HashMap<String, Vec<ColumnMeta>>> {
        if let Ok(meta) = self.meta.lock()
            && let Some(map) = meta.get(conn_id)
            && self.entry_fresh(conn_id)
        {
            return Some(map.clone());
        }
        self.schedule_load(conn_id);
        None
    }

    /// 【#53】该连接的 L1 条目是否新鲜（未超 [`COMPLETION_CACHE_TTL_SECS`]）。
    ///
    /// 条目缺失 = 不新鲜（未载入过）；时间源用 `SystemTime`（与写回同一基准）。
    /// 锁失败按不新鲜处理——命中检查路径宁可触发一次重载，也不给陈旧的候选。
    fn entry_fresh(&self, conn_id: &str) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        self.cache_meta
            .lock()
            .map(|meta| meta.get(conn_id).map(|entry| entry.fresh(now)).unwrap_or(false))
            .unwrap_or(false)
    }

    /// 起一次后台预载（**同一连接只起一次**；完成前重复调用都是 no-op）
    fn schedule_load(&self, conn_id: &str) {
        {
            let Ok(mut loading) = self.loading.lock() else {
                return;
            };
            if !loading.insert(conn_id.to_string()) {
                return; // 已经在载了
            }
        }
        let project_root = self
            .shared
            .project_root()
            .map(|root| root.to_string_lossy().to_string());
        let loaded = self.loaded.clone();
        let keys = self.keys.clone();
        let meta = self.meta.clone();
        let cache_meta = self.cache_meta.clone();
        let generation = self.generation.clone();
        let loading = self.loading.clone();
        let warm_submitted = self.warm_submitted.clone();
        let conn_id = conn_id.to_string();
        std::thread::spawn(move || {
            // 【#53】取本次载入代际：写回前若发现已有更高代（期间被重载过）→ 丢弃本次结果。
            // 当前 `loading` 已挡同连接并发，此分支防御未来放开并发载入时的「旧覆盖新」。
            let load_gen = generation.fetch_add(1, Ordering::Relaxed) + 1;
            // 【T2a-2 / P1】一次遍历同时产出候选目录 / 主键映射 / 列元数据（同一份导航缓存，零额外 I/O）
            let (catalog, keys_map, meta_map) = load_raw_catalog(&conn_id, project_root.as_deref());
            if catalog.objects.is_empty() && keys_map.is_empty() {
                // 【§710】NavCache 没有对象（导航从未展开过 / 预热只暖了 schema）→
                // 提交一次整条连接的实时内省（cache-aside 写回 L2，复用导航队列的
                // 信号量 / 让位），本次如实给空；填好后下一次按键就有候选。
                let mut submitted = warm_submitted.lock().unwrap_or_else(|poison| poison.into_inner());
                if submitted.insert(conn_id.clone()) {
                    database::nav_jobs::enqueue_warm_completion(&conn_id, project_root.as_deref());
                }
            } else {
                // 命中对象 → 允许未来再内省（元数据变化后重新提交）
                if let Ok(mut submitted) = warm_submitted.lock() {
                    submitted.remove(&conn_id);
                }
            }
            // 【#53】写回前代际比对：已有更高代（期间另一次载入已写回）→ 本次结果作废
            {
                let stale = cache_meta
                    .lock()
                    .map(|meta| {
                        meta.get(&conn_id)
                            .map(|entry| entry.generation > load_gen)
                            .unwrap_or(false)
                    })
                    .unwrap_or(false);
                if stale {
                    if let Ok(mut loading) = loading.lock() {
                        loading.remove(&conn_id);
                    }
                    return;
                }
            }
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            if let Ok(mut loaded) = loaded.lock() {
                loaded.insert(conn_id.clone(), catalog);
            }
            if let Ok(mut slot) = keys.lock() {
                slot.insert(conn_id.clone(), keys_map);
            }
            if let Ok(mut slot) = meta.lock() {
                slot.insert(conn_id.clone(), meta_map);
            }
            if let Ok(mut slot) = cache_meta.lock() {
                slot.insert(
                    conn_id.clone(),
                    CacheEntryMeta {
                        generation: load_gen,
                        loaded_at_secs: now,
                    },
                );
            }
            if let Ok(mut loading) = loading.lock() {
                loading.remove(&conn_id);
            }
        });
    }
}

impl CompletionPort for WorkbenchCompletion {
    fn catalog(&self, conn_id: Option<&str>, channel: ExecChannel, schema: Option<&str>) -> Catalog {
        let Some(conn_id) = conn_id else {
            // 未绑定连接：执行时跟随“当前连接”，但补全不该猜是哪条（那正是最容易给错的地方）
            return Catalog::default();
        };
        if channel != ExecChannel::Federated {
            // 源库 / 本地加速：`schema.表`（加速档不写限定名也好用，但写了更明确）
            return self
                .raw(conn_id)
                .map(|catalog| filter_catalog_by_schema(catalog, schema))
                .unwrap_or_default();
        }

        // 联邦：只给**已挂上**的源，并按别名限定
        let Some(snapshot) = fed_session::snapshot_for(conn_id) else {
            return Catalog::default();
        };
        let mut catalog = Catalog::default();
        for entry in snapshot.sources.iter().filter(|entry| entry.is_ready()) {
            add_federated_source(&self, entry, &mut catalog);
        }
        catalog
    }

    /// 【B9 切片二】模板片段（内存读：启动时那份预载；没载好就是空）
    fn templates(&self) -> Vec<TemplateSnippet> {
        self.templates
            .lock()
            .map(|slot| slot.clone())
            .unwrap_or_default()
    }

    /// 【T2a-2】表的主键列（内存读：与候选目录同一次预载；没载好就是空）
    ///
    /// 联邦通道不探测：联邦是只读副本（写回先被编辑判据的 `ChannelReadOnly` 拦下），
    /// 探测了也用不上。`table` 是 `default_table_name` 猜出来的裸表名。
    fn primary_key(&self, conn_id: Option<&str>, channel: ExecChannel, table: &str) -> Option<Vec<String>> {
        if channel == ExecChannel::Federated {
            return None;
        }
        let conn_id = conn_id?;
        let map = self.keys_map(conn_id)?;
        lookup_primary_key(&map, table)
    }

    /// 【P1】结果集字段注释（血缘 + 表列元数据；内存读：与候选目录同一次预载）
    ///
    /// 逐级兜底，缺哪一级都**如实给空**（渲染只画列名，不影响结果）：
    /// - 未绑定连接 / 联邦通道（SQL 跨源写别名限定，血缘不适用）/ 未知驱动方言 → 全 `None`；
    /// - `projection_sources` 解析失败（非 SELECT / 语法错）→ 全 `None`；
    /// - 列元数据没载好 → 安排一次预载（幂等），本次给空；
    /// - 来源表找不到 / 列不在表列集里（表达式列、缓存列集滞后）→ 那一列 `None`。
    fn column_annotations(
        &self,
        conn_id: Option<&str>,
        channel: ExecChannel,
        sql: &str,
        column_count: usize,
    ) -> Vec<Option<ColumnAnnotation>> {
        let Some(conn_id) = conn_id else {
            return vec![None; column_count];
        };
        if matches!(channel, ExecChannel::Federated) {
            return vec![None; column_count];
        }
        // 方言从连接驱动 id 推（结果实际跑在 `conn_id` 上，不能用“当前选中连接”）
        let Some(dialect) = self
            .shared
            .connections
            .borrow()
            .iter()
            .find(|item| item.id == conn_id)
            .and_then(|item| driver_to_dialect(&item.driver))
        else {
            return vec![None; column_count];
        };
        let Ok(sources) = engine::SqlEngine::projection_sources(sql, dialect) else {
            return vec![None; column_count];
        };
        let Some(meta_map) = self.meta_map(conn_id) else {
            return vec![None; column_count];
        };
        let mut out: Vec<Option<ColumnAnnotation>> = Vec::new();
        for source in sources {
            match source {
                ProjectionSource::Column { table, column } => {
                    out.push(lookup_column(&meta_map, table.as_deref(), &column));
                }
                ProjectionSource::Star { table } => match table.as_deref() {
                    // `SELECT *`：按表列集展开（实体表；子查询 / 派生表没有 schema 对应列）
                    Some(table) => match find_table(&meta_map, table) {
                        Some((key, columns)) => out
                            .extend(columns.iter().map(|meta| Some(meta.to_annotation(&key)))),
                        None => out.push(None),
                    },
                    None => out.push(None),
                },
                ProjectionSource::Expression => out.push(None),
            }
            // 展开超了（缓存列集比真实结果多）截断；`column_count` 是权威
            if out.len() >= column_count {
                break;
            }
        }
        // 展开不够（表达式列 / 缓存列集比真实结果少）补足 None
        out.extend(std::iter::repeat_n(None, column_count.saturating_sub(out.len())));
        out
    }
}

/// 【P1】驱动 id → SQL 方言（`projection_sources` 用）
///
/// 只映射**本仓实证存在**的驱动 id（连接对话框 / 测试夹具里见过：`mysql` / `mysql_native` /
/// `postgres` / `postgres_native` / `sqlite` / `duckdb`）；其余驱动（未来接入）如实 `None` →
/// 该连接的结果没有字段注释，而不是拿错方言解析出错。
fn driver_to_dialect(driver: &str) -> Option<SqlDialect> {
    match driver {
        "mysql" | "mysql_native" => Some(SqlDialect::Mysql),
        "postgres" | "postgres_native" => Some(SqlDialect::Postgres),
        "sqlite" => Some(SqlDialect::Sqlite),
        "duckdb" => Some(SqlDialect::Duckdb),
        _ => None,
    }
}

/// 【P1】表列集查找：限定名直接命中；裸名跨 schema 兜底（首个表名相同的）
///
/// 与 [`lookup_primary_key`] 同款两步：`projection_sources` 归位可能给裸名
/// （FROM 没写 schema），而映射的键是导航缓存的限定名（`public.orders`）。
/// 跨 schema 同名表取首个命中（罕见；DBeaver 的 JDBC 按默认 schema 解析，同样只取一个）。
fn find_table<'a>(
    meta_map: &'a HashMap<String, Vec<ColumnMeta>>,
    table: &str,
) -> Option<(String, &'a Vec<ColumnMeta>)> {
    if let Some(columns) = meta_map.get(table) {
        return Some((table.to_string(), columns));
    }
    meta_map
        .iter()
        .find(|(key, _)| key.rsplit('.').next() == Some(table))
        .map(|(key, columns)| (key.clone(), columns))
}

/// 【P1】按来源表 + 来源列查列元数据（来源表找不到 / 列不在列集 = `None`）
fn lookup_column(
    meta_map: &HashMap<String, Vec<ColumnMeta>>,
    table: Option<&str>,
    column: &str,
) -> Option<ColumnAnnotation> {
    let table = table?;
    let (key, columns) = find_table(meta_map, table)?;
    columns
        .iter()
        .find(|meta| meta.name == column)
        .map(|meta| meta.to_annotation(&key))
}

/// 【T2a-2】在主键映射里按表名找键：限定名直取（`schema.表`）→ 裸名匹配（首个命中）
///
/// 为什么两步：`default_table_name` 猜出来的是裸表名（`orders`），而映射的键是
/// 导航缓存的限定名（`public.orders`）。同库不同 schema 下重名罕见，取第一个命中即可；
/// 真遇到重名，写回落错表的风险由「编辑判据还要过项目只读闸」再兜一层，不在这里猜。
fn lookup_primary_key(keys: &HashMap<String, Vec<String>>, table: &str) -> Option<Vec<String>> {
    if let Some(columns) = keys.get(table) {
        return Some(columns.clone());
    }
    keys.iter()
        .find(|(qualified, _)| qualified.rsplit('.').next() == Some(table))
        .map(|(_, columns)| columns.clone())
}

/// 把一个已挂上的源并进联邦候选（`别名.schema.表`；L2 源按两段名）
fn add_federated_source(port: &WorkbenchCompletion, entry: &MountedSource, into: &mut Catalog) {
    let alias = entry.alias();
    let Some(raw) = port.raw(&entry.source.conn_id) else {
        return;
    };
    // L2（Oracle 这类）的表挂在附加源的 `main` schema 下 → 两段名（架构 §2.1）
    let two_part = entry.source.kind.needs_secret();
    for object in raw.objects {
        let tail = if two_part {
            object.label.rsplit('.').next().unwrap_or(&object.label).to_string()
        } else {
            object.label.clone()
        };
        into.objects.push(Candidate {
            label: format!("{alias}.{tail}"),
            detail: object.detail,
            kind: object.kind,
        });
    }
    for (table, column, detail) in raw.columns {
        into.columns.push((format!("{alias}.{table}"), column, detail));
    }
    into.truncated |= raw.truncated;
}

/// 【P1】预载的表列元数据（与主键映射同一遍遍历；字段注释的原料）
#[derive(Debug, Clone)]
struct ColumnMeta {
    name: String,
    data_type: Option<String>,
    nullable: Option<bool>,
    default_value: Option<String>,
    comment: Option<String>,
}

impl ColumnMeta {
    /// 转成编辑器端的注释（`table` = 查找命中的真实限定名）
    fn to_annotation(&self, table: &str) -> ColumnAnnotation {
        ColumnAnnotation {
            source_table: Some(table.to_string()),
            source_column: Some(self.name.clone()),
            comment: self.comment.clone(),
            data_type: self.data_type.clone(),
            nullable: self.nullable,
            default_value: self.default_value.clone(),
        }
    }
}

/// 从导航缓存装一份原始目录 + 主键映射 + 列元数据（**后台线程里跑**：这里允许 I/O）
///
/// 第二返回值是 **`schema.表` → 主键列** 的映射（写回的行定位依据，T2a-2）；
/// 第三返回值是 **`schema.表` → 列元数据**（字段注释原料，P1）。
/// 三份同一次遍历（同一份列数据），只是在列上多读几个位，零额外 I/O。
fn load_raw_catalog(
    conn_id: &str,
    project_root: Option<&str>,
) -> (Catalog, HashMap<String, Vec<String>>, HashMap<String, Vec<ColumnMeta>>) {
    let Some(cache) = NavCache::open(conn_id, project_root) else {
        // 缓存不可用（项目连接缺项目根 / 打开失败）→ 没有候选，如实空着
        return (Catalog::default(), HashMap::new(), HashMap::new());
    };
    let mut catalog = Catalog::default();
    let mut keys: HashMap<String, Vec<String>> = HashMap::new();
    let mut meta: HashMap<String, Vec<ColumnMeta>> = HashMap::new();
    for (_, schema, schema_id) in cache.all_schemas() {
        for want_view in [false, true] {
            let Some(objects) = cache.objects(schema_id, want_view) else {
                continue;
            };
            for (name, comment) in objects {
                let kind = if want_view {
                    CandidateKind::View
                } else {
                    CandidateKind::Table
                };
                let mut candidate = Candidate::new(format!("{schema}.{name}"), kind);
                if let Some(comment) = comment.filter(|c| !c.is_empty()) {
                    candidate = candidate.with_detail(comment);
                }
                catalog.objects.push(candidate);
                if let Some(columns) = cache.columns(schema_id, &name) {
                    let qualified = format!("{schema}.{name}");
                    let mut column_meta = Vec::with_capacity(columns.len());
                    for column in columns {
                        catalog
                            .columns
                            .push((qualified.clone(), column.name.clone(), Some(column.data_type.clone())));
                        // 【T2a-2】主键列按表结构顺序收集（NavCache 的列序即 ordinal）
                        if column.is_primary_key {
                            keys.entry(qualified.clone()).or_default().push(column.name.clone());
                        }
                        // 【P1】列元数据（字段注释原料：注释 / 类型 / 可空 / 默认）
                        column_meta.push(ColumnMeta {
                            name: column.name.clone(),
                            data_type: Some(column.data_type.clone()),
                            nullable: Some(column.nullable),
                            default_value: column.default_value.clone(),
                            comment: column.comment.clone(),
                        });
                    }
                    meta.insert(qualified, column_meta);
                }
                if catalog.objects.len() >= MAX_OBJECTS || catalog.columns.len() >= MAX_COLUMNS {
                    catalog.truncated = true;
                    return (catalog, keys, meta);
                }
            }
        }
    }
    (catalog, keys, meta)
}

/// 【B9 切片二】把模板片段从库里读出来（**后台线程里跑**：这里允许 I/O）
///
/// 读不到（全局库未就绪 / 查询失败）就返回空：菜单不摆那一项，而不是报一个用户看不懂的错。
fn load_templates() -> Vec<TemplateSnippet> {
    let Some(manager) = engine::migration::get_global_db_manager() else {
        return Vec::new();
    };
    let Ok(store) = manager.get_sql_template_store() else {
        return Vec::new();
    };
    let Ok(rows) = store.get_all() else {
        return Vec::new();
    };
    rows.into_iter().map(to_snippet).collect()
}

/// 库里的行 → 编辑器的片段（只留名字 / 内容 / 说明）
fn to_snippet(template: engine::persistence::sql_template_store::SqlTemplate) -> TemplateSnippet {
    TemplateSnippet {
        id: template.id,
        name: template.name,
        content: template.content,
        description: template.description.filter(|text| !text.is_empty()),
    }
}

/// 把补全端口接到编辑器共享状态上（**启动装配调用一次**）
pub fn attach(shared: &editor::shared::EditorShared, workbench: &Shared) {
    let port = WorkbenchCompletion::new(workbench.clone());
    // 模板预载一次（内存读给菜单用；读不到就空着）
    port.schedule_template_load();
    shared.attach_completion(Rc::new(port));
}

#[cfg(test)]
mod tests {
    // 安全模式：**不通配导入**
    use super::{
        driver_to_dialect, find_table, load_raw_catalog, lookup_column, lookup_primary_key,
        CacheEntryMeta, WorkbenchCompletion, COMPLETION_CACHE_TTL_SECS,
    };
    use std::collections::HashMap;
    use database::cache::NavCache;
    use editor::channel::ExecChannel;
    use editor::completion::{CandidateKind, CompletionPort};
    use engine::driver::traits::{ColumnDetail, NodeInfo, SchemaObjectKind};
    use project::ui::OpenProject;
    use workbench_shell::model::ConnectionItem;

    use crate::panels::Shared;

    fn temp_root(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("rds_completion_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("建目录");
        dir
    }

    /// 轮询等到后台线程落笔（`schedule_load` 是 `thread::spawn`，无句柄可 join；
    /// 落笔点是 `loaded` / `warm_submitted` 的写回）。
    fn wait_until(cond: impl Fn() -> bool, what: &str) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while !cond() {
            assert!(std::time::Instant::now() < deadline, "后台线程未在 3s 内{what}");
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }

    fn seed(conn_id: &str, root: &std::path::Path) {
        let root_s = root.to_string_lossy().to_string();
        let mut cache = NavCache::open(conn_id, Some(&root_s)).expect("打开缓存");
        cache.put_schemas("main", &["public".to_string()]);
        let sid = cache.schema_id("main", "public").expect("schema_id");
        cache.put_objects(
            sid,
            &[
                NodeInfo::new("orders", SchemaObjectKind::Table)
                    .with_comment(Some("订单".to_string())),
                NodeInfo::new("order_stats", SchemaObjectKind::View),
            ],
        );
        cache.put_columns(
            sid,
            "orders",
            &[
                ColumnDetail {
                    name: "id".to_string(),
                    data_type: "integer".to_string(),
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
                },
                ColumnDetail {
                    name: "total".to_string(),
                    data_type: "numeric".to_string(),
                    nullable: true,
                    is_primary_key: false,
                    is_foreign_key: false,
                    default_value: None,
                    comment: Some("总额".to_string()),
                    is_identity: false,
                    identity_generation: None,
                    collation: None,
                    extra: Default::default(),
                    references: None,
                    ordinal: 1,
                },
            ],
        );
        cache.rebuild_index("main", "public");
    }

    /// 装出来的原始目录：表 / 视图分得清、列带类型、限定名是 `schema.表`
    #[test]
    fn the_raw_catalog_comes_from_the_navigation_cache() {
        let root = temp_root("raw");
        let root_s = root.to_string_lossy().to_string();
        seed("P_completion_raw", &root);

        let (catalog, keys, meta) = load_raw_catalog("P_completion_raw", Some(&root_s));
        assert_eq!(catalog.objects.len(), 2, "{:?}", catalog.objects);
        assert_eq!(catalog.objects[0].label, "public.orders");
        assert_eq!(catalog.objects[0].kind, CandidateKind::Table);
        assert_eq!(catalog.objects[0].detail.as_deref(), Some("订单"));
        assert_eq!(catalog.objects[1].kind, CandidateKind::View);
        assert_eq!(
            catalog.columns,
            vec![
                ("public.orders".to_string(), "id".to_string(), Some("integer".to_string())),
                ("public.orders".to_string(), "total".to_string(), Some("numeric".to_string()))
            ]
        );
        assert!(!catalog.truncated);

        // 【T2a-2】主键映射：主键列按表结构顺序收；非主键列不收；视图没列 → 无映射
        assert_eq!(keys.get("public.orders"), Some(&vec!["id".to_string()]), "{keys:?}");
        assert_eq!(keys.get("public.order_stats"), None, "视图没有主键列");

        // 【P1】列元数据：注释 / 类型 / 可空 / 默认与 NavCache 列一致（视图没列 → 无映射）
        let orders = meta.get("public.orders").expect("orders 列元数据");
        assert_eq!(orders.len(), 2);
        assert_eq!(orders[0].name, "id");
        assert_eq!(orders[0].data_type.as_deref(), Some("integer"));
        assert_eq!(orders[0].nullable, Some(false));
        assert_eq!(orders[1].nullable, Some(true), "total 可空");
        assert!(meta.get("public.order_stats").is_none(), "视图没有列元数据");

        // 缓存不可用（项目连接缺项目根）→ 空目录，不崩
        assert!(load_raw_catalog("P_completion_raw", None).0.is_empty());

        let _ = std::fs::remove_dir_all(&root);
    }

    /// 【T2a-2】按表名找主键：限定名直取；裸名匹配任意 schema；没有 = `None`
    #[test]
    fn primary_key_lookup_prefers_qualified_then_falls_back_to_bare_name() {
        let mut keys = HashMap::new();
        keys.insert("public.orders".to_string(), vec!["id".to_string()]);
        keys.insert("audit.orders".to_string(), vec!["batch_id".to_string()]);

        // 限定名直取
        assert_eq!(
            lookup_primary_key(&keys, "public.orders"),
            Some(vec!["id".to_string()])
        );
        // 裸名匹配 → 第一个命中（迭代序不保证，但两者都是 orders，取到任一个都成立）
        let bare = lookup_primary_key(&keys, "orders").expect("裸名应命中");
        assert!(
            bare == vec!["id".to_string()] || bare == vec!["batch_id".to_string()],
            "裸名 orders 应命中两个 schema 之一，实际 {bare:?}"
        );
        // 没有这张表
        assert_eq!(lookup_primary_key(&keys, "nope"), None);
        // 空映射
        assert_eq!(lookup_primary_key(&HashMap::new(), "orders"), None);
    }

    /// 【B9 切片二】库里的行 → 编辑器片段：只留名字 / 内容 / 说明，空说明当“无”
    #[test]
    fn a_template_row_becomes_a_snippet_without_the_storage_fields() {
        let row = engine::persistence::sql_template_store::SqlTemplate {
            id: "builtin_select_all".to_string(),
            name: "查询所有记录".to_string(),
            content: "SELECT * FROM {table};".to_string(),
            db_type: None,
            category: "查询".to_string(),
            description: Some(String::new()),
            tags: None,
            is_builtin: true,
            created_at_ms: 0,
            updated_at_ms: 0,
        };
        let snippet = super::to_snippet(row);
        assert_eq!(snippet.name, "查询所有记录");
        assert_eq!(snippet.content, "SELECT * FROM {table};");
        assert_eq!(snippet.description, None, "空说明不该摆一个空字符串");
    }

    // ---------- 【P1】字段注释 ----------

    /// 【P1】驱动 id → 方言：实证的 6 个 id 命中，其余如实 `None`（不拿错方言解析）
    #[test]
    fn driver_to_dialect_maps_known_drivers_only() {
        use engine::sql::SqlDialect;
        assert_eq!(driver_to_dialect("sqlite"), Some(SqlDialect::Sqlite));
        assert_eq!(driver_to_dialect("mysql"), Some(SqlDialect::Mysql));
        assert_eq!(driver_to_dialect("mysql_native"), Some(SqlDialect::Mysql));
        assert_eq!(driver_to_dialect("postgres"), Some(SqlDialect::Postgres));
        assert_eq!(driver_to_dialect("postgres_native"), Some(SqlDialect::Postgres));
        assert_eq!(driver_to_dialect("duckdb"), Some(SqlDialect::Duckdb));
        // 未实证的驱动：如实 None
        assert_eq!(driver_to_dialect("oracle"), None);
        assert_eq!(driver_to_dialect(""), None);
    }

    /// 【P1】表列集查找：限定名直取；裸名跨 schema 兜底；没有 = `None`
    #[test]
    fn find_table_prefers_qualified_then_falls_back_to_bare_name() {
        let mut meta = HashMap::new();
        meta.insert("public.orders".to_string(), Vec::new());
        meta.insert("audit.orders".to_string(), Vec::new());

        let (key, _) = find_table(&meta, "public.orders").expect("限定名直取");
        assert_eq!(key, "public.orders");
        let (key, _) = find_table(&meta, "orders").expect("裸名兜底");
        assert!(key == "public.orders" || key == "audit.orders", "{key}");
        assert!(find_table(&meta, "nope").is_none());
        assert!(find_table(&HashMap::new(), "orders").is_none());
    }

    /// 【P1】lookup_column：限定名 / 裸名兜底 / 未命中逐级如实
    #[test]
    fn lookup_column_matches_qualified_then_bare_then_none() {
        let mut meta = HashMap::new();
        meta.insert(
            "public.orders".to_string(),
            vec![super::ColumnMeta {
                name: "id".into(),
                data_type: Some("integer".into()),
                nullable: Some(false),
                default_value: None,
                comment: Some("订单号".into()),
            }],
        );
        let ann = lookup_column(&meta, Some("public.orders"), "id").expect("限定名命中");
        assert_eq!(ann.source_table.as_deref(), Some("public.orders"));
        assert_eq!(ann.comment.as_deref(), Some("订单号"));
        let ann = lookup_column(&meta, Some("orders"), "id").expect("裸名兜底");
        assert_eq!(ann.source_table.as_deref(), Some("public.orders"), "来源显示真实限定名");
        assert_eq!(lookup_column(&meta, Some("orders"), "nope"), None);
        assert_eq!(lookup_column(&meta, None, "id"), None);
        assert_eq!(lookup_column(&HashMap::new(), Some("orders"), "id"), None);
    }

    /// 【P1】端口：单表 SELECT 血缘命中（注释 / 可空从 NavCache 来）；表达式 / 非 SELECT /
    /// 未绑定连接 / 列集滞后逐级如实给空
    #[test]
    fn column_annotations_resolve_single_table_select() {
        let root = temp_root("p1");
        seed("P1_annotations", &root);
        let shared = Shared::with_connections(
            vec![ConnectionItem {
                id: "P1_annotations".into(),
                name: "测试库".into(),
                driver: "sqlite".into(),
                connected: true,
                host: None,
                port: None,
                database: None,
                schema: None,
                description: None,
                use_duckdb_fed: false,
                created_at: String::new(),
                updated_at: String::new(),
            }],
            None,
        );
        // 项目根要指到 seed 的缓存目录：NavCache::open 缺项目根打不开缓存
        *shared.project.borrow_mut() = Some(OpenProject::new(root.clone(), "P1"));
        let port = WorkbenchCompletion::new(shared);

        // 预载落笔前：第一次 `meta_map` 触发后台预载；轮询等到载好
        wait_until(|| port.meta_map("P1_annotations").is_some(), "载好列元数据");

        // 单表裸名 SELECT：两列都有注释，来源归位到限定名 public.orders
        let anns = port.column_annotations(
            Some("P1_annotations"),
            ExecChannel::Source,
            "SELECT id, total FROM orders",
            2,
        );
        assert_eq!(anns.len(), 2);
        let first = anns[0].clone().expect("id 有注释");
        assert_eq!(first.source_table.as_deref(), Some("public.orders"));
        assert_eq!(first.source_column.as_deref(), Some("id"));
        assert_eq!(first.data_type.as_deref(), Some("integer"));
        assert_eq!(first.nullable, Some(false));
        let second = anns[1].clone().expect("total 有注释");
        assert_eq!(second.comment.as_deref(), Some("总额"), "列注释来自 NavCache");
        assert_eq!(second.nullable, Some(true));

        // `SELECT *`：按表列集展开（2 列）
        let anns = port.column_annotations(
            Some("P1_annotations"),
            ExecChannel::Source,
            "SELECT * FROM orders",
            2,
        );
        assert_eq!(anns.len(), 2);
        assert_eq!(
            anns[0].as_ref().and_then(|a| a.source_column.as_deref()),
            Some("id")
        );

        // 表达式列：没有来源 → 那一列 None
        let anns = port.column_annotations(
            Some("P1_annotations"),
            ExecChannel::Source,
            "SELECT count(*) AS n FROM orders",
            1,
        );
        assert_eq!(anns, vec![None]);

        // 非 SELECT：解析失败 → 全 None（不影响结果）
        let anns = port.column_annotations(
            Some("P1_annotations"),
            ExecChannel::Source,
            "INSERT INTO orders(id) VALUES (1)",
            1,
        );
        assert_eq!(anns, vec![None]);

        // 未绑定连接 → 全 None
        let anns = port.column_annotations(None, ExecChannel::Source, "SELECT id FROM orders", 1);
        assert_eq!(anns, vec![None]);

        // 列不在表列集里（缓存列集滞后）→ 那一列 None
        let anns = port.column_annotations(
            Some("P1_annotations"),
            ExecChannel::Source,
            "SELECT missing FROM orders",
            1,
        );
        assert_eq!(anns, vec![None]);

        let _ = std::fs::remove_dir_all(&root);
    }

    /// 端口：未绑定连接给空；未载过时先给空并安排预载（这一拍不阻塞）
    #[test]
    fn the_port_returns_empty_until_the_preload_lands() {
        let shared = Shared::with_connections(Vec::new(), None);
        let port = WorkbenchCompletion::new(shared);

        assert!(
            port.catalog(None, ExecChannel::Source).is_empty(),
            "未绑定连接不给候选（不猜是哪条）"
        );
        // 第一次问：内存里没有 → 空目录（后台开始载；这里只断言不阻塞、不 panic）
        assert!(port.catalog(Some("P_nope"), ExecChannel::Source).is_empty());
        // 再问一次仍给空（预载还在跑或已失败），但不该 panic
        assert!(port.catalog(Some("P_nope"), ExecChannel::Source).is_empty());
    }

    /// 【§710】空 NavCache（导航从未展开过 / 预热只暖 schema）→ 补全未命中时提交
    /// 一次实时内省：`warm_submitted` 记录该连接（insert 与投递在同一闭包内，
    /// 记上即已投递）。后台内省对空连接管理器快速失败也无害——补全继续只给
    /// 关键字与函数（如实，不假装有元数据）。
    #[test]
    fn an_empty_cache_submits_one_warm_introspection() {
        let root = temp_root("warm_submit");
        let shared = Shared::with_connections(Vec::new(), None);
        // 项目根就位（NavCache 能打开）但没有种子对象 → 走「空目录 → 投递内省」分支
        *shared.project.borrow_mut() = Some(OpenProject::new(root.clone(), "t"));
        let port = WorkbenchCompletion::new(shared);

        port.schedule_load("P_warm_710");
        wait_until(
            || {
                port.warm_submitted
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .contains("P_warm_710")
            },
            "记录 warm_submitted",
        );

        // 同连接再调度：`loading` 去重（同一连接只起一次）→ 不重复投递；
        // 标记仍在（未命中就不移除，防「每敲一字投一次内省」的串行重试）。
        port.schedule_load("P_warm_710");
        std::thread::sleep(std::time::Duration::from_millis(80));
        assert!(
            port.warm_submitted
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .contains("P_warm_710"),
            "空缓存未命中时标记必须保留"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    /// 【§710】NavCache 命中对象 → 移除 `warm_submitted`（连接元数据变化后允许再内省）。
    #[test]
    fn a_hit_cache_clears_the_warm_marker() {
        let root = temp_root("warm_hit");
        seed("P_warm_hit_710", &root);
        let shared = Shared::with_connections(Vec::new(), None);
        *shared.project.borrow_mut() = Some(OpenProject::new(root.clone(), "t"));
        let port = WorkbenchCompletion::new(shared);

        port.schedule_load("P_warm_hit_710");
        wait_until(
            || {
                port.loaded
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .contains_key("P_warm_hit_710")
            },
            "写入 loaded",
        );

        assert!(
            !port
                .warm_submitted
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .contains("P_warm_hit_710"),
            "命中缓存后标记要移除（允许未来元数据变化后再内省）"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    /// 【#53】TTL 边界：恰好 TTL 秒内新鲜，超过即过期（blanco 同款：过期 = 未命中）
    #[test]
    fn cache_entry_fresh_boundary() {
        let entry = CacheEntryMeta {
            generation: 1,
            loaded_at_secs: 1_000,
        };
        assert!(entry.fresh(1_000), "同一秒新鲜");
        assert!(
            entry.fresh(1_000 + COMPLETION_CACHE_TTL_SECS),
            "恰好 TTL 仍新鲜（含边界）"
        );
        assert!(
            !entry.fresh(1_000 + COMPLETION_CACHE_TTL_SECS + 1),
            "超过 TTL 即过期"
        );
        // 时钟回拨（异常环境）：saturating_sub 保证不 panic，仍视为新鲜
        assert!(entry.fresh(999), "时钟回拨仍新鲜");
    }

    /// 【#53】L1 不再「载入一次永不刷新」：过期条目触发重载，代际递增、时间戳刷新。
    #[test]
    fn an_expired_entry_reloads_with_a_new_generation() {
        let root = temp_root("ttl");
        seed("P_ttl_53", &root);
        let shared = Shared::with_connections(Vec::new(), None);
        *shared.project.borrow_mut() = Some(OpenProject::new(root.clone(), "t"));
        let port = WorkbenchCompletion::new(shared);

        // 首次命中：等预载落笔，记下第一代
        wait_until(|| port.raw("P_ttl_53").is_some(), "首次载好目录");
        let first_gen = {
            let meta = port.cache_meta.lock().unwrap_or_else(|p| p.into_inner());
            meta.get("P_ttl_53").expect("条目存在").generation
        };
        assert_eq!(first_gen, 1, "首次载入代际 = 1");

        // 模拟 TTL 过期：把时间戳拨回 TTL 之外（不改代际——过期判定只看时间戳）
        {
            let mut meta = port.cache_meta.lock().unwrap_or_else(|p| p.into_inner());
            let entry = meta.get_mut("P_ttl_53").expect("条目存在");
            entry.loaded_at_secs =
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs() as i64)
                    .unwrap_or(0)
                    - COMPLETION_CACHE_TTL_SECS
                    - 60;
        }

        // 过期 → 视为未命中（本次给空）并触发重载；等重载落笔
        assert!(
            port.raw("P_ttl_53").is_none(),
            "过期条目不再直接给（宁缺毋滥，下一键刷新）"
        );
        wait_until(|| port.raw("P_ttl_53").is_some(), "重载后再次命中");

        let second_gen = {
            let meta = port.cache_meta.lock().unwrap_or_else(|p| p.into_inner());
            meta.get("P_ttl_53").expect("条目存在").generation
        };
        assert!(
            second_gen > first_gen,
            "重载代际递增（{first_gen} → {second_gen}），证明不是拿旧条目应付"
        );
        // 重载后时间戳回到 TTL 内（否则下一次 raw 又判过期）
        assert!(port.entry_fresh("P_ttl_53"), "重载后条目新鲜");

        let _ = std::fs::remove_dir_all(&root);
    }
}

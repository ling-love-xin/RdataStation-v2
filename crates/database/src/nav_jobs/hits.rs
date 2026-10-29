//! 搜索命中模型与转换：统一命中形状（[`SearchHit`]）与名称 / 内容检索的纯函数。
//! 无状态、无 I/O（仅缓存存在性探测）；任务队列见 [`worker`]。

use crate::model::ObjectRef;
use crate::nav::family_profile::family_search_scope;
use engine::persistence::FtsSearchResult;
pub use engine::persistence::NameMatchMode;
use engine::ObjectKind;

pub const SEARCH_LIMIT_PER_CONN: usize = 50;

/// 一次搜索的总命中上限（多连接时防止“30 个连接 × 50 条”把命中列表与渲染拖垮）。
pub const SEARCH_MAX_HITS: usize = 300;

/// 一次索引搜索的并行度：目标连接数按此分块，每块一条 scoped 线程直开临时缓存连接。
///
/// 经验值（2026-10-14 拍板）：8 块既能把 30+ 连接的搜索摊到多核，又不至于为每次搜索
/// 开几十条线程（每条都要建连 + 跑 PRAGMA + 迁移校验，线程数不等于加速）。
pub const SEARCH_PARALLEL: usize = 8;

/// 一次索引搜索的目标连接（宿主可见且通过 facet 筛选的连接）。
#[derive(Clone, Debug)]
pub struct SearchTarget {
    pub conn_id: String,
    /// 连接显示名（命中行要显示“是哪个连接”）。
    pub label: String,
    /// 驱动 id（命中行要开属性面板，需要它标注驱动）。
    pub driver: String,
}

/// 一条命中的来源维度（D11 why 三态：名称 / 注释 / 定义）。
///
/// 名称档原生命中记 [`HitSource::Name`]；注释 / 定义开关并入的 FTS 候选分别记
/// [`HitSource::Comment`] / [`HitSource::Definition`]，视图侧据此在副标区打来源徽标。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum HitSource {
    /// 名称匹配（默认）。
    #[default]
    Name,
    /// 注释匹配（FTS 注释语料）。
    Comment,
    /// 定义匹配（FTS 定义语料；本批内省链路尚未采集定义，候选恒为空）。
    Definition,
}

/// 搜索档：名称（`metadata_index` 中缀）与内容（FTS5，注释 / 数据类型）。
///
/// 两档不是同一种结果形态：名称档给短名字列表，内容档要让用户看到**为什么命中**
/// （`snippet` 带命中标记），因此 `#` 前缀单独一档。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SearchKind {
    /// 名称档：`metadata_index` + LIKE 中缀（不受长度限制）。
    Name,
    /// 内容档：`metadata_fts`（trigram，**至少 3 个字符**，不足必无命中）。
    FullText,
}

/// 一条搜索命中（已展开成视图可直接渲染的形状）。
#[derive(Clone, Debug)]
pub struct SearchHit {
    pub conn_id: String,
    pub conn_label: String,
    pub driver: String,
    /// 类别：`table` / `view` / `schema` / `column` / `routine`。
    pub object_type: String,
    pub object_name: String,
    /// 列命中时的所属表名。
    pub parent_name: Option<String>,
    pub catalog: Option<String>,
    pub schema: Option<String>,
    /// 内容档命中片段（带 `<mark>` 标记）；名称档为 `None`。
    pub snippet: Option<String>,
    /// 命中来源维度（D11 why 三态）：名称 / 注释 / 定义。
    pub hit_source: HitSource,
}

impl SearchHit {
    /// 命中对象的**身份**：`{连接}/{catalog}/{schema}/{父对象}/{名字}`（`ObjectRef::key` 口径）。
    ///
    /// 为什么要单独一处：导航结果行的业务键（`NavRow::key`，虚拟列表里那一行的唯一身份 /
    /// 键盘漫游锚点）与元素 id（GPUI 要求同层唯一）都要它，两边各拼一份必有一份会漂移。
    /// key 与树节点同构，于是「结果区那一行」与「树上那一行」说的是同一个对象。
    ///
    /// 索引里出现**不可寻址**的类别串时退化为「连接-类别-名字」：这类命中渲染得出来
    /// （见 `nav_object_type_label` 的兜底），只是定位不了——身份仍要唯一，不能返回空串。
    pub(crate) fn key(&self) -> String {
        let object = ObjectRef::from_index_hit(
            &self.conn_id,
            &self.object_type,
            &self.object_name,
            self.parent_name.as_deref(),
            self.catalog.as_deref(),
            self.schema.as_deref(),
        );
        match object {
            Some(object) => object.key(),
            None => format!("{}-{}-{}", self.conn_id, self.object_type, self.object_name),
        }
    }
}

/// 一次搜索的完整结果（一个批次涵盖全部目标连接）。
pub struct SearchResult {
    /// 本批次的消费方（诊断用；分发改由槽位承担，见 [`SearchConsumer`]）。
    pub consumer: SearchConsumer,
    /// 发起批次序号：视图侧据此丢弃**过期批次**（新一批已发出，旧批迟到覆盖）。
    pub seq: u64,
    /// 本次搜索词；回填方据此丢弃**过期批次**（用户已经把词改了）。
    pub query: String,
    /// 实际搜了的连接数（有缓存的那些；无缓存的连接不建文件、直接跳过）。
    pub searched: usize,
    pub hits: Vec<SearchHit>,
}

/// 索引搜索的**消费方**：同一条后台通道被多个入口用（导航面板的树顶结果行、
/// Quick Open 浮层、元数据搜索面板），结果必须按消费方分发——`drain` 是「取走」语义，
/// 不分流就是谁先取谁得，另一个入口会永远拿到空结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SearchConsumer {
    /// 数据源导航面板（结果行在导航列表最前）。
    Navigator,
    /// Quick Open 浮层。
    QuickOpen,
    /// 元数据搜索面板（中央「搜索: query」tab，2026-10-14 P0）。
    Panel,
}

impl SearchConsumer {
    /// 槽位数量（`search_results` / `pending_search` 两张表共用）。
    pub(crate) const COUNT: usize = 3;

    /// 内部槽位下标。
    pub(crate) fn ix(self) -> usize {
        match self {
            SearchConsumer::Navigator => 0,
            SearchConsumer::QuickOpen => 1,
            SearchConsumer::Panel => 2,
        }
    }
}

pub(crate) fn nav_split_two_segment(query: &str) -> (Option<&str>, Option<&str>) {
    if query.matches('.').count() != 1 {
        return (None, None);
    }
    match query.split_once('.') {
        Some((a, b)) if !a.is_empty() && !b.is_empty() => (Some(a), Some(b)),
        _ => (None, None),
    }
}

/// 搜索一个连接块（单条 scoped 线程内执行；纯函数，便于确定性单测）。
///
/// 返回 `(搜了的连接数, 命中)`：无落盘缓存的连接直接跳过（搜索不建文件）；
/// 每连接命中封顶 [`SEARCH_LIMIT_PER_CONN`]（全局 300 截断由调用方在合并后统一做）。
///
/// `case_sensitive` / `regex`（搜索框 `Aa` / `.*` 开关）：名称档在 **LIKE 候选**上
/// 再做 Rust 侧精筛——SQLite `LIKE` 对 ASCII 不区分大小写，候选是超集，精筛不会漏；
/// 正则同理叠加在候选集上（LIKE 已要求整词中缀命中，故 `^ord` 这类**锚定**模式与
/// `.` 通配任意字符的语义**不完整**——这是「中缀 + 精筛」架构的已知边界，面板 UI 已注明）。
/// 正则编译失败（非法模式）时退化为不精筛（日志留痕），避免一次手误让整个搜索报废。
///
/// `schema.object` 两段：名称段走索引档，归属段在候选上按 `schema` / `catalog`
/// 精筛（`Aa` / `.*` 精筛同样只作用于名称段——归属段是系统事实，不做模糊）。
///
/// D11 注释 / 定义并入：`include_comments` 开启后，名称档额外并入 FTS 注释候选，
/// 按族档案 `comments` 集合过滤类型、按身份 `key()` 去重；`include_definitions` 本批
/// 内省链路尚未采集定义，开关不产生候选（字段先透传，采集链路接通后在此并入）。
#[allow(clippy::too_many_arguments)]
pub(crate) fn search_targets(
    kind: SearchKind,
    query: &str,
    case_sensitive: bool,
    regex: bool,
    name_mode: NameMatchMode,
    include_comments: bool,
    include_definitions: bool,
    project_root: Option<&str>,
    targets: &[SearchTarget],
) -> (usize, Vec<SearchHit>) {
    let mut searched = 0usize;
    let mut hits = Vec::new();
    for target in targets {
        // 没落盘缓存的连接直接跳过：搜索不建文件（见 `cache::cache_file_exists`）。
        if !crate::cache::cache_file_exists(&target.conn_id, project_root) {
            continue;
        }
        let Some(cache) = crate::cache::NavCache::open_direct(&target.conn_id, project_root) else {
            continue;
        };
        searched += 1;
        match kind {
            SearchKind::Name => {
                let (segment_schema, segment_name) = nav_split_two_segment(query);
                // 两段时名称段单独做索引档（整串含 `.` 在对象名里永远中缀不中）。
                let needle = segment_name.unwrap_or(query);

                // 名称候选：按选定匹配方式走索引档。
                for hit in cache.search_index(needle, SEARCH_LIMIT_PER_CONN, name_mode) {
                    hits.push(SearchHit {
                        conn_id: target.conn_id.clone(),
                        conn_label: target.label.clone(),
                        driver: target.driver.clone(),
                        object_type: hit.object_type,
                        object_name: hit.object_name,
                        parent_name: hit.parent_name,
                        catalog: hit.catalog_name,
                        schema: hit.schema_name,
                        snippet: None,
                        hit_source: HitSource::Name,
                    });
                }
                // 归属段精筛：schema 或 catalog 任一精确命中即可（MySQL `库.表`）。
                if let Some(schema) = segment_schema {
                    hits.retain(|h| {
                        h.schema.as_deref() == Some(schema)
                            || h.catalog.as_deref() == Some(schema)
                    });
                }
                // 「Aa」区分大小写：`contains` 精筛（Rust 字符串比较区分大小写）。
                if case_sensitive {
                    hits.retain(|h| h.object_name.contains(needle));
                }
                // 「.*」正则：正则精筛（`(?i)` 前缀与「Aa」关联动；编译失败退化为不筛）。
                if regex {
                    let pattern = if case_sensitive {
                        needle.to_string()
                    } else {
                        format!("(?i){needle}")
                    };
                    match regex::Regex::new(&pattern) {
                        Ok(re) => hits.retain(|h| re.is_match(&h.object_name)),
                        Err(e) => {
                            tracing::warn!(
                                pattern = %pattern,
                                error = %e,
                                "导航搜索正则编译失败，本轮退化为名称匹配"
                            );
                        }
                    }
                }

                // 注释候选并入：按族 comments 集合过滤 FTS 类型，业务身份去重后并入名称档。
                if include_comments {
                    let scope = family_search_scope(&target.driver);
                    // 去重键用业务身份四元组（类别 / 父对象 / 名字 / schema），不拼 catalog：
                    // 名称索引行带 `catalog_name`、FTS 行 catalog 恒 `None`，`key()` 会把同一
                    // 对象因 catalog 填充差异判成两个身份；去重只认四元组即可识别重复。
                    let known: std::collections::BTreeSet<(String, Option<String>, String, Option<String>)> =
                        hits
                            .iter()
                            .map(|h| {
                                (
                                    h.object_type.clone(),
                                    h.parent_name.clone(),
                                    h.object_name.clone(),
                                    h.schema.clone(),
                                )
                            })
                            .collect();
                    for fts in cache.search_fts(needle, SEARCH_LIMIT_PER_CONN) {
                        let Some(fts_kind) = fts_type_to_object_kind(&fts.search_type) else {
                            continue;
                        };
                        if !scope.comments.contains(&fts_kind) {
                            continue;
                        }
                        let hit = fts_hit_to_search_hit(target, fts, HitSource::Comment);
                        let identity = (
                            hit.object_type.clone(),
                            hit.parent_name.clone(),
                            hit.object_name.clone(),
                            hit.schema.clone(),
                        );
                        if !known.contains(&identity) {
                            hits.push(hit);
                        }
                    }
                }
                // 定义候选：本批内省链路尚未采集视图 / 例程定义，开关不产生候选；
                // 采集链路接通后在此按 `scope.definitions` 过滤并入（与注释候选同构）。
                let _ = include_definitions;
            }
            SearchKind::FullText => {
                for hit in cache.search_fts(query, SEARCH_LIMIT_PER_CONN) {
                    hits.push(fts_hit_to_search_hit(target, hit, HitSource::Name));
                }
            }
        }
    }
    (searched, hits)
}

/// FTS `search_type` 字符串 → 身份类别（注释候选按族 `comments` 集合过滤时用）。
///
/// 取值与 `rebuild_fts_schema` 写入侧一致：`table` / `view` / `column` / `routine` /
/// `schema` / `catalog`；未识别串返回 `None`（不进注释候选，安全兜底）。
pub(crate) fn fts_type_to_object_kind(search_type: &str) -> Option<ObjectKind> {
    match search_type {
        "table" => Some(ObjectKind::Table),
        "view" => Some(ObjectKind::View),
        "column" => Some(ObjectKind::Column),
        "routine" => Some(ObjectKind::Routine),
        "schema" => Some(ObjectKind::Schema),
        "catalog" => Some(ObjectKind::Catalog),
        _ => None,
    }
}

/// S2-② 迟到 stale guard 判定（纯函数，fluxDB `connection_tree.rs:305-344` 参照）：
/// 任务在排队 / 执行期间连接被删除、断开或改配置 → 结果为**过期**，回填方应丢弃。
///
/// 指纹规则：发起/执行时的快照是 `Some` 才比对（`None` = 连接未注册 / 不可共享，
/// 跳过指纹项）；存在性 / 连接态永远比对（它们是权威来源）。
/// FTS 命中 → 统一命中形状（纯映射，便于单测）。
///
/// 两处不对称要当心：
/// - FTS 行的 `schema_name` / `parent_name` 是**空串**（不是 NULL），转成 `Option` 时要过滤；
/// - FTS 表里没有 catalog（名称档靠 JOIN `schemata` 拿），所以内容档的 `catalog` 为 `None`。
pub(crate) fn fts_hit_to_search_hit(
    target: &SearchTarget,
    hit: FtsSearchResult,
    source: HitSource,
) -> SearchHit {
    let non_empty = |s: String| if s.is_empty() { None } else { Some(s) };
    SearchHit {
        conn_id: target.conn_id.clone(),
        conn_label: target.label.clone(),
        driver: target.driver.clone(),
        object_type: hit.search_type,
        object_name: hit.object_name,
        parent_name: non_empty(hit.parent_name),
        catalog: None,
        schema: non_empty(hit.schema_name),
        snippet: Some(hit.snippet),
        hit_source: source,
    }
}


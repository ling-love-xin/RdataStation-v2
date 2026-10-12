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

use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use editor::channel::ExecChannel;
use editor::completion::{Candidate, CandidateKind, Catalog, CompletionPort, TemplateSnippet};

use database::cache::NavCache;
use engine::duckdb::federation::{registry::MountedSource, session as fed_session};

use crate::panels::Shared;

/// 单条连接最多装多少表 / 视图（超了标记 `truncated`，不再往下装）
const MAX_OBJECTS: usize = 3_000;
/// 单条连接最多装多少列
const MAX_COLUMNS: usize = 30_000;

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

    /// 原始目录（命中即给；未命中就安排一次后台预载，本次给空）
    fn raw(&self, conn_id: &str) -> Option<Catalog> {
        if let Ok(loaded) = self.loaded.lock()
            && let Some(catalog) = loaded.get(conn_id)
        {
            return Some(catalog.clone());
        }
        self.schedule_load(conn_id);
        None
    }

    /// 【T2a-2】主键映射（命中即给；未命中就安排一次后台预载，本次给空）
    ///
    /// 与 [`Self::raw`] 同一策略：预载是**一次性的**（`loaded` / `keys` 同线程同遍历），
    /// 这里未命中时 `schedule_load` 已在上一次 `raw` 命中检查里去重——重复调用是 no-op。
    fn keys_map(&self, conn_id: &str) -> Option<HashMap<String, Vec<String>>> {
        if let Ok(keys) = self.keys.lock()
            && let Some(map) = keys.get(conn_id)
        {
            return Some(map.clone());
        }
        self.schedule_load(conn_id);
        None
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
        let loading = self.loading.clone();
        let conn_id = conn_id.to_string();
        std::thread::spawn(move || {
            // 【T2a-2】一次遍历同时产出候选目录与主键映射（同一份导航缓存，零额外 I/O）
            let (catalog, keys_map) = load_raw_catalog(&conn_id, project_root.as_deref());
            if let Ok(mut loaded) = loaded.lock() {
                loaded.insert(conn_id.clone(), catalog);
            }
            if let Ok(mut slot) = keys.lock() {
                slot.insert(conn_id.clone(), keys_map);
            }
            if let Ok(mut loading) = loading.lock() {
                loading.remove(&conn_id);
            }
        });
    }
}

impl CompletionPort for WorkbenchCompletion {
    fn catalog(&self, conn_id: Option<&str>, channel: ExecChannel) -> Catalog {
        let Some(conn_id) = conn_id else {
            // 未绑定连接：执行时跟随“当前连接”，但补全不该猜是哪条（那正是最容易给错的地方）
            return Catalog::default();
        };
        if channel != ExecChannel::Federated {
            // 源库 / 本地加速：`schema.表`（加速档不写限定名也好用，但写了更明确）
            return self.raw(conn_id).unwrap_or_default();
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

/// 从导航缓存装一份原始目录 + 主键映射（**后台线程里跑**：这里允许 I/O）
///
/// 第二返回值是 **`schema.表` → 主键列** 的映射（写回的行定位依据，T2a-2）；与候选目录
/// 同一次遍历（同一份列数据），只在列上多读一个 `is_primary_key` 位，零额外 I/O。
fn load_raw_catalog(
    conn_id: &str,
    project_root: Option<&str>,
) -> (Catalog, HashMap<String, Vec<String>>) {
    let Some(cache) = NavCache::open(conn_id, project_root) else {
        // 缓存不可用（项目连接缺项目根 / 打开失败）→ 没有候选，如实空着
        return (Catalog::default(), HashMap::new());
    };
    let mut catalog = Catalog::default();
    let mut keys: HashMap<String, Vec<String>> = HashMap::new();
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
                    for column in columns {
                        catalog
                            .columns
                            .push((qualified.clone(), column.name.clone(), Some(column.data_type)));
                        // 【T2a-2】主键列按表结构顺序收集（NavCache 的列序即 ordinal）
                        if column.is_primary_key {
                            keys.entry(qualified.clone()).or_default().push(column.name);
                        }
                    }
                }
                if catalog.objects.len() >= MAX_OBJECTS || catalog.columns.len() >= MAX_COLUMNS {
                    catalog.truncated = true;
                    return (catalog, keys);
                }
            }
        }
    }
    (catalog, keys)
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
    use super::{WorkbenchCompletion, load_raw_catalog, lookup_primary_key};
    use std::collections::HashMap;
    use database::cache::NavCache;
    use editor::channel::ExecChannel;
    use editor::completion::{CandidateKind, CompletionPort};
    use engine::driver::traits::{ColumnDetail, NodeInfo, SchemaObjectKind};

    use crate::panels::Shared;

    fn temp_root(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("rds_completion_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("建目录");
        dir
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
                    comment: None,
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

        let (catalog, keys) = load_raw_catalog("P_completion_raw", Some(&root_s));
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
}

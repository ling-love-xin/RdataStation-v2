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
use editor::completion::{Candidate, CandidateKind, Catalog, CompletionPort};

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
    /// 正在载的 conn_id（去重：同一连接同时只载一次）
    loading: Arc<Mutex<HashSet<String>>>,
}

impl WorkbenchCompletion {
    pub fn new(shared: Shared) -> Self {
        Self {
            shared,
            loaded: Arc::new(Mutex::new(HashMap::new())),
            loading: Arc::new(Mutex::new(HashSet::new())),
        }
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
        let loading = self.loading.clone();
        let conn_id = conn_id.to_string();
        std::thread::spawn(move || {
            let catalog = load_raw_catalog(&conn_id, project_root.as_deref());
            if let Ok(mut loaded) = loaded.lock() {
                loaded.insert(conn_id.clone(), catalog);
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

/// 从导航缓存装一份原始目录（**后台线程里跑**：这里允许 I/O）
fn load_raw_catalog(conn_id: &str, project_root: Option<&str>) -> Catalog {
    let Some(cache) = NavCache::open(conn_id, project_root) else {
        // 缓存不可用（项目连接缺项目根 / 打开失败）→ 没有候选，如实空着
        return Catalog::default();
    };
    let mut catalog = Catalog::default();
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
                    for column in columns {
                        catalog
                            .columns
                            .push((format!("{schema}.{name}"), column.name, Some(column.data_type)));
                    }
                }
                if catalog.objects.len() >= MAX_OBJECTS || catalog.columns.len() >= MAX_COLUMNS {
                    catalog.truncated = true;
                    return catalog;
                }
            }
        }
    }
    catalog
}

/// 把补全端口接到编辑器共享状态上（**启动装配调用一次**）
pub fn attach(shared: &editor::shared::EditorShared, workbench: &Shared) {
    shared.attach_completion(Rc::new(WorkbenchCompletion::new(workbench.clone())));
}

#[cfg(test)]
mod tests {
    // 安全模式：**不通配导入**
    use super::{WorkbenchCompletion, load_raw_catalog};
    use database::cache::NavCache;
    use editor::channel::ExecChannel;
    use editor::completion::{CandidateKind, CompletionPort};
    use engine::driver::traits::{ColumnDetail, SchemaObject, SchemaObjectKind};

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
                SchemaObject {
                    name: "orders".to_string(),
                    kind: SchemaObjectKind::Table,
                    children: None,
                    comment: Some("订单".to_string()),
                    table_name: None,
                    event: None,
                },
                SchemaObject {
                    name: "order_stats".to_string(),
                    kind: SchemaObjectKind::View,
                    children: None,
                    comment: None,
                    table_name: None,
                    event: None,
                },
            ],
        );
        cache.put_columns(
            sid,
            "orders",
            &[ColumnDetail {
                name: "total".to_string(),
                data_type: "numeric".to_string(),
                nullable: true,
                is_primary_key: false,
                is_foreign_key: false,
                default_value: None,
                comment: None,
                extra: Default::default(),
            }],
        );
        cache.rebuild_index("main", "public");
    }

    /// 装出来的原始目录：表 / 视图分得清、列带类型、限定名是 `schema.表`
    #[test]
    fn the_raw_catalog_comes_from_the_navigation_cache() {
        let root = temp_root("raw");
        let root_s = root.to_string_lossy().to_string();
        seed("P_completion_raw", &root);

        let catalog = load_raw_catalog("P_completion_raw", Some(&root_s));
        assert_eq!(catalog.objects.len(), 2, "{:?}", catalog.objects);
        assert_eq!(catalog.objects[0].label, "public.orders");
        assert_eq!(catalog.objects[0].kind, CandidateKind::Table);
        assert_eq!(catalog.objects[0].detail.as_deref(), Some("订单"));
        assert_eq!(catalog.objects[1].kind, CandidateKind::View);
        assert_eq!(
            catalog.columns,
            vec![(
                "public.orders".to_string(),
                "total".to_string(),
                Some("numeric".to_string())
            )]
        );
        assert!(!catalog.truncated);

        // 缓存不可用（项目连接缺项目根）→ 空目录，不崩
        assert!(load_raw_catalog("P_completion_raw", None).is_empty());

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
}

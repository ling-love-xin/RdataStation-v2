//! 库 / schema 列表端口的工作台实现（T2c-1）：状态栏面包屑的可切项
//!
//! ## 数据从哪来
//!
//! `database::cache::NavCache`（导航侧 L2 缓存：schema / 表 / 视图 / 列）——与
//! `editor_completion` 同一数据源：面包屑要的是**快 + 稳**，不实时内省。
//! 缓存里没有的连接 = 空列表 → 状态栏不显示可切的面包屑（如实降级，不造项）。
//!
//! ## 为什么是「预载 + 内存读」
//!
//! 状态栏每帧都会问一次（面包屑菜单弹出时也读）——**渲染路径不能做 I/O**，
//! 所以这里与 `WorkbenchCompletion` 同一策略：
//!
//! 1. 命中内存缓存 → 直接给；
//! 2. 没命中 → **起一次后台线程**装该连接的库 / schema 清单（去重：一条连接同时只载一次），
//!    本次先给空；
//! 3. 载好之后下一次渲染就有可切项了（不通知：面包屑晚一拍是常态）。

use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use crate::connection::{SchemaInfo, SchemasPort};
use crate::host::EditorHost;

use database::cache::NavCache;

/// 工作台实现：库 / schema 清单（内存读；首次未命中时后台预载）
pub struct WorkbenchSchemas {
    /// 项目根来源（NavCache 的打开参数：项目连接的缓存挂在项目根下）
    host: Rc<dyn EditorHost>,
    /// 已载好的清单（按 conn_id）：渲染路径只读快照
    loaded: Arc<Mutex<HashMap<String, Vec<SchemaInfo>>>>,
    /// 正在载的 conn_id（去重：同一连接同时只载一次）
    loading: Arc<Mutex<HashSet<String>>>,
}

impl WorkbenchSchemas {
    pub fn new(host: Rc<dyn EditorHost>) -> Self {
        Self {
            host,
            loaded: Arc::new(Mutex::new(HashMap::new())),
            loading: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    /// 清单（命中即给；未命中就安排一次后台预载，本次给空）
    fn raw(&self, conn_id: &str) -> Option<Vec<SchemaInfo>> {
        if let Ok(loaded) = self.loaded.lock()
            && let Some(list) = loaded.get(conn_id)
        {
            return Some(list.clone());
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
            .host
            .project_root()
            .map(|root| root.to_string_lossy().to_string());
        let loaded = self.loaded.clone();
        let loading = self.loading.clone();
        let conn_id = conn_id.to_string();
        std::thread::spawn(move || {
            let list = load_schemas(&conn_id, project_root.as_deref());
            if let Ok(mut slot) = loaded.lock() {
                slot.insert(conn_id.clone(), list);
            }
            if let Ok(mut slot) = loading.lock() {
                slot.remove(&conn_id);
            }
        });
    }
}

impl SchemasPort for WorkbenchSchemas {
    fn schemas(&self, conn_id: &str) -> Vec<SchemaInfo> {
        self.raw(conn_id).unwrap_or_default()
    }
}

/// 从导航缓存读库 / schema 清单（**后台线程里跑**：这里允许 I/O）
///
/// `all_schemas()` 返回 `(catalog, schema, schema_id)`：catalog 段在无 Catalog 层的驱动
/// （MySQL 侧库即 schema）为空——那里状态栏面包屑就不显示库段（如实，不编）。
fn load_schemas(conn_id: &str, project_root: Option<&str>) -> Vec<SchemaInfo> {
    let Some(cache) = NavCache::open(conn_id, project_root) else {
        return Vec::new();
    };
    cache
        .all_schemas()
        .into_iter()
        .map(|(catalog, schema, _)| SchemaInfo {
            catalog: (!catalog.is_empty()).then_some(catalog),
            schema,
        })
        .collect()
}

/// 把 schema 端口接到编辑器共享状态上（**启动装配调用一次**）
pub fn attach(shared: &crate::shared::EditorShared, host: Rc<dyn EditorHost>) {
    shared.attach_schemas(Rc::new(WorkbenchSchemas::new(host)));
}

#[cfg(test)]
mod tests {
    // 安全模式：**不通配导入**
    use super::load_schemas;
    use crate::connection::SchemaInfo;
    use database::cache::NavCache;

    fn temp_root(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("rds_schema_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("建目录");
        dir
    }

    /// 从导航缓存读库 / schema 清单：有 catalog 层的驱动带库段，无 catalog 层（MySQL 侧
    /// 库即 schema）库段为空——两种都要如实映射
    #[test]
    fn load_schemas_maps_catalog_and_schema_segments() {
        let root = temp_root("map");
        let root_s = root.to_string_lossy().to_string();
        let cache = NavCache::open("P_s", Some(&root_s)).expect("打开缓存");
        cache.put_schemas("rdb", &["public".to_string(), "analytics".to_string()]);
        drop(cache);

        // 顺序不是契约（list_schemas 不保证插入序）：按 schema 名排好后比较
        let mut list = load_schemas("P_s", Some(&root_s));
        list.sort_by(|a, b| a.schema.cmp(&b.schema));
        assert_eq!(
            list,
            vec![
                SchemaInfo {
                    catalog: Some("rdb".to_string()),
                    schema: "analytics".to_string(),
                },
                SchemaInfo {
                    catalog: Some("rdb".to_string()),
                    schema: "public".to_string(),
                },
            ]
        );

        // 连接不认识 → 空列表（面包屑没可切项 → 降级为文本连接段）
        assert!(load_schemas("P_ghost", Some(&root_s)).is_empty());
    }

    /// 无 Catalog 层的驱动（MySQL 侧库即 schema）：catalog 段如实留空，不编一个库出来
    #[test]
    fn catalog_segment_stays_empty_without_a_catalog_layer() {
        let root = temp_root("nocat");
        let root_s = root.to_string_lossy().to_string();
        let cache = NavCache::open("P_nc", Some(&root_s)).expect("打开缓存");
        cache.put_schemas("", &["mydb".to_string()]);
        drop(cache);

        let list = load_schemas("P_nc", Some(&root_s));
        assert_eq!(
            list,
            vec![SchemaInfo {
                catalog: None,
                schema: "mydb".to_string(),
            }]
        );
    }
}

//! 表结构端口的工作台实现（B7 切片三）：导出 `SQL Updates` 时的键列
//!
//! ## 数据从哪来
//!
//! `database::cache::NavCache`（导航侧 L2 缓存：schema / 表 / 视图 / 列）——与
//! `editor_schema` / `editor_completion` 同一数据源。键列 = 缓存里表的 `is_primary_key`
//! 列（导航对象展开表时已内省过，列属性含主键标记）。
//!
//! ## 为什么这里可以直接读、不必「预载 + 后台」
//!
//! `editor_schema` 走预载，是因为状态栏**每帧**都问。键列查询只在**导出菜单弹出时**
//! 问一次（用户显式动作，事件路径）——同步读 L2 缓存文件可接受（几十 ms 级），
//! 预载反而无从下手（表名任意，无法预载“所有表的键”）。
//!
//! ## 找不到怎么办
//!
//! 连接不认识 / 表不认识 / 表没有主键列 → `None`：面板**不摆** `SQL Updates`，
//! 不猜“拿第一列当键”（`UPDATE … WHERE 随便一列` 猜错的代价是改错行）。

use std::rc::Rc;

use editor::export::TableKeysPort;

use database::cache::NavCache;

use crate::panels::Shared;

/// 工作台实现：从导航缓存读表的主键列
pub struct WorkbenchTableKeys {
    shared: Shared,
}

impl WorkbenchTableKeys {
    pub fn new(shared: Shared) -> Self {
        Self { shared }
    }
}

impl TableKeysPort for WorkbenchTableKeys {
    fn primary_key_columns(&self, conn_id: &str, table: &str) -> Option<Vec<String>> {
        let project_root = self
            .shared
            .project_root()
            .map(|root| root.to_string_lossy().to_string());
        primary_key_columns(conn_id, table, project_root.as_deref())
    }
}

/// 从导航缓存读某表的主键列（可单测的纯函数）
///
/// 表可能出现在任意 schema（SQL 可能是 `SELECT … FROM orders` 跨 schema 查询）：
/// 逐个 schema 找第一个“认识这张表”的（`columns()` 返回非空即命中），
/// 取其中主键列；没有主键列也返回 `None`（宁可回绝也不猜）。
fn primary_key_columns(
    conn_id: &str,
    table: &str,
    project_root: Option<&str>,
) -> Option<Vec<String>> {
    let cache = NavCache::open(conn_id, project_root)?;
    for (_, _, schema_id) in cache.all_schemas() {
        let Some(columns) = cache.columns(schema_id, table) else {
            continue;
        };
        let keys: Vec<String> = columns
            .iter()
            .filter(|column| column.is_primary_key)
            .map(|column| column.name.clone())
            .collect();
        if !keys.is_empty() {
            return Some(keys);
        }
    }
    None
}

/// 把表结构端口接到编辑器共享状态上（**启动装配调用一次**）
pub fn attach(shared: &editor::shared::EditorShared, workbench: &Shared) {
    shared.attach_table_keys(Rc::new(WorkbenchTableKeys::new(workbench.clone())));
}

#[cfg(test)]
mod tests {
    // 安全模式：**不通配导入**
    use super::primary_key_columns;
    use database::cache::NavCache;
    use engine::driver::traits::ColumnDetail;

    fn temp_root(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("rds_keys_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("建目录");
        dir
    }

    /// 表的主键列从导航缓存读出（列属性里的主键标记）
    #[test]
    fn reads_primary_key_columns_from_nav_cache() {
        let root = temp_root("pk");
        let root_s = root.to_string_lossy().to_string();
        let cache = NavCache::open("P_pk", Some(&root_s)).expect("打开缓存");
        cache.put_schemas("rdb", &["public".to_string()]);
        cache.mark_schema_complete("rdb", "public");
        let schema_id = cache.schema_id("rdb", "public").expect("schema_id");
        // 表要先注册（put_objects），columns 的 get_table_id 才找得到
        cache.put_objects(
            schema_id,
            &[engine::driver::traits::NodeInfo::new(
                "orders",
                engine::driver::traits::SchemaObjectKind::Table,
            )],
        );
        cache.put_columns(
            schema_id,
            "orders",
            &[ColumnDetail {
                name: "id".to_string(),
                data_type: "INTEGER".to_string(),
                nullable: false,
                is_primary_key: true,
                is_foreign_key: false,
                default_value: None,
                comment: None,
                is_identity: false,
                identity_generation: None,
                collation: None,
                extra: std::collections::HashMap::new(),
                references: None,
                ordinal: 1,
            }],
        );
        drop(cache);

        assert_eq!(
            primary_key_columns("P_pk", "orders", Some(&root_s)),
            Some(vec!["id".to_string()]),
            "主键列要原样按缓存给出"
        );
    }

    /// 连接不认识 / 表不认识 / 表无主键 → 一律 `None`（宁可回绝也不猜）
    #[test]
    fn unknown_or_keyless_tables_return_none() {
        let root = temp_root("nokey");
        let root_s = root.to_string_lossy().to_string();
        let cache = NavCache::open("P_nk", Some(&root_s)).expect("打开缓存");
        cache.put_schemas("rdb", &["public".to_string()]);
        cache.mark_schema_complete("rdb", "public");
        let schema_id = cache.schema_id("rdb", "public").expect("schema_id");
        cache.put_objects(
            schema_id,
            &[engine::driver::traits::NodeInfo::new(
                "events",
                engine::driver::traits::SchemaObjectKind::Table,
            )],
        );
        cache.put_columns(
            schema_id,
            "events",
            &[ColumnDetail {
                name: "seq".to_string(),
                data_type: "INTEGER".to_string(),
                nullable: false,
                is_primary_key: false,
                is_foreign_key: false,
                default_value: None,
                comment: None,
                is_identity: false,
                identity_generation: None,
                collation: None,
                extra: std::collections::HashMap::new(),
                references: None,
                ordinal: 1,
            }],
        );
        drop(cache);

        assert_eq!(primary_key_columns("P_ghost", "orders", Some(&root_s)), None);
        assert_eq!(primary_key_columns("P_nk", "orders", Some(&root_s)), None);
        assert_eq!(
            primary_key_columns("P_nk", "events", Some(&root_s)),
            None,
            "有表但没主键列也不给——没有键的 UPDATE 只能猜"
        );
    }
}

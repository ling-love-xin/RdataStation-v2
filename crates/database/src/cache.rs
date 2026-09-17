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

use engine::driver::traits::{ColumnDetail, SchemaObject, SchemaObjectKind};
use engine::persistence::{ConnectionType, MetadataCacheManager, MetadataCacheOps};

use crate::model::NavSource;

/// 缓存中表示「视图」的 `table_type`（与 [`NavCache::put_objects`] 的写入约定一致）。
const TABLE_TYPE_VIEW: &str = "VIEW";
/// 缓存中表示「表」的 `table_type`（受 `tables.table_type` CHECK 约束限制，取值见迁移 `004_refactor_to_normalized.sql`）。
const TABLE_TYPE_BASE: &str = "TABLE";

/// 连接级导航缓存句柄（持有一个缓存 SQLite 连接）。
pub struct NavCache {
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
        let manager = MetadataCacheManager::new(conn_id, connection_type, project_root).ok()?;
        let conn = manager.open().ok()?;
        Some(Self {
            ops: MetadataCacheOps::new(conn),
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

    /// 回写 schema 名（须先于对象写入，后续按名取 `schema_id`）。
    pub fn put_schemas(&self, catalog: &str, names: &[String]) {
        for name in names {
            let _ = self.ops.save_schema(catalog, name, None, None);
        }
    }

    /// 按名取 `schema_id`。
    pub fn schema_id(&self, catalog: &str, schema: &str) -> Option<i64> {
        self.ops.get_schema_id(catalog, schema).ok().flatten()
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

    /// 回写表 / 视图对象。
    pub fn put_objects(&self, schema_id: i64, objects: &[SchemaObject]) {
        for obj in objects {
            let is_view = obj.kind == SchemaObjectKind::View;
            let table_type = if is_view {
                TABLE_TYPE_VIEW
            } else {
                TABLE_TYPE_BASE
            };
            if let Ok(table_id) = self.ops.save_table(
                schema_id,
                &obj.name,
                table_type,
                obj.comment.as_deref(),
                None,
                None,
            ) {
                if is_view {
                    let _ = self.ops.save_view(table_id, "", None, None);
                }
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
            let _ = self.ops.save_column(
                table_id,
                &col.name,
                &col.data_type,
                ix as i32,
                col.nullable,
                col.is_primary_key,
                false,
                col.default_value.as_deref(),
                col.comment.as_deref(),
            );
        }
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
            &[SchemaObject {
                name: "t1".to_string(),
                kind: SchemaObjectKind::Table,
                children: None,
                comment: Some("表注释".to_string()),
                table_name: None,
                event: None,
            }],
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
        cache.put_objects(
            sid,
            &[SchemaObject {
                name: "t_old".to_string(),
                kind: SchemaObjectKind::Table,
                children: None,
                comment: None,
                table_name: None,
                event: None,
            }],
        );
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

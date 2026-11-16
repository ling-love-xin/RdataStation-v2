//! 数据库根的懒加载树：缓存「已向连接层问过的那一层」，只把**已加载的部分**拍平成行。
//!
//! ## 为什么是缓存而不是每次重查
//!
//! 老的实现是「扁平索引字段 + 渐进单列」：`selected_conn` / `catalogs` / `selected_catalog` /
//! `schemas` / `selected_schema` / `tables`——一次只有**一条路径**活着。树要求多个分支
//! 同时展开，所以必须按 `NodeId` 缓存每一层，否则每帧都要重新 introspect。
//!
//! ## 深度预算（§01：深度上限 3；列不做节点）
//!
//! 连接在第 1 层，再往下只剩两层，而 `conn → catalog → schema → table` 是四层。规则：
//!
//! | 情形 | 树的形状 |
//! | --- | --- |
//! | 单 catalog（绝大多数连接） | 连接(1) → **schema(2)** → 表(3)，**跳过 catalog 行** |
//! | 多 catalog | 连接(1) → catalog(2) → 表(3)，**schema 折进表标签**（`schema · 表`） |
//! | 驱动无独立 schema 层（MySQL 口径 `schema.is_empty()`） | 连接(1) → 表(2) |
//!
//! 被折叠的那一层必须在**标签里交代清楚**：否则两个 schema 下的同名表在树上无法区分
//! （这正是老渲染 `format!("{} · {}", table.schema, table.name)` 已有的口径，沿用不改）。
//!
//! ## 空结果也要记
//!
//! `put` 连空 `Vec` 一起写：否则「展开了一个空 schema」在下一帧又会被判定成「没加载过」，
//! 于是每次展开都重新 introspect 一次——这是模态内 `block_on` 卡顿的典型来源。

use std::collections::HashMap;

use super::node::{NodeId, NodeKind, Row};
use super::TableCandidate;

/// 已加载的子行缓存：父节点 → 子行。
///
/// 深度与标签在**加载时**定好（`catalog_level` / `schema_rows` / `table_rows`），
/// 渲染只读——渲染层不做任何深度换算，避免两处各算一遍。
#[derive(Debug, Default)]
pub struct DbCache {
    kids: HashMap<NodeId, Vec<Row>>,
}

impl DbCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_loaded(&self, id: &NodeId) -> bool {
        self.kids.contains_key(id)
    }

    /// 记录某一层的加载结果（空结果同样要记，见模块文档末段）。
    pub fn put(&mut self, id: NodeId, rows: Vec<Row>) {
        self.kids.insert(id, rows);
    }

    pub fn children(&self, id: &NodeId) -> Option<&[Row]> {
        self.kids.get(id).map(|v| v.as_slice())
    }

    /// 连接断开 / 换库时整体作废：缓存里的表名可能已经不存在了。
    pub fn clear(&mut self) {
        self.kids.clear();
    }
}

/// 连接行的最小输入（刻意不引 `ConnectionItem`，`db.rs` 只认数据）。
pub struct ConnBrief<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub driver: &'a str,
    pub host: &'a str,
}

/// 连接行（depth 1）。元信息给驱动与主机——与老渲染的 `名称 — 驱动 (host)` 同源。
pub fn conn_row(brief: &ConnBrief<'_>) -> Row {
    let mut row = Row::new(
        NodeId::Conn(brief.id.to_string()),
        1,
        NodeKind::Conn,
        brief.name,
    );
    let meta = if brief.host.is_empty() {
        brief.driver.to_string()
    } else {
        format!("{} · {}", brief.driver, brief.host)
    };
    if !meta.is_empty() {
        row = row.with_meta(meta);
    }
    row
}

/// `catalog` 层的加载结果：要么「只有一个，直接跳过这一层」，要么「多个，逐行列出」。
///
/// 用枚举而不是 `Vec<Row>`：调用方需要知道「要不要再问一层 schema」——单 catalog 时
/// catalog 名要作为下文上下文继续传，多 catalog 时每个 catalog 各自成行、互不干扰。
#[derive(Debug, Clone, PartialEq)]
pub enum CatalogLevel {
    /// 无 catalog 或无有效 catalog，调用方不应再往下问
    None,
    /// 只有一个 catalog：**不产行**，把它当作下文上下文
    Single(String),
    /// 多个 catalog：逐行列出（depth 2，可再展开到表）
    Many(Vec<Row>),
}

/// 解析 catalog 清单。
pub fn catalog_level(conn: &str, catalogs: &[String], depth: u8) -> CatalogLevel {
    match catalogs.len() {
        0 => CatalogLevel::None,
        1 => CatalogLevel::Single(catalogs[0].clone()),
        _ => CatalogLevel::Many(
            catalogs
                .iter()
                .map(|c| {
                    Row::new(
                        NodeId::DbCatalog {
                            conn: conn.to_string(),
                            catalog: c.clone(),
                        },
                        depth,
                        NodeKind::Folder,
                        c.clone(),
                    )
                })
                .collect(),
        ),
    }
}

/// schema 行（单 catalog 情形，depth 2）。
///
/// `schemas` 为空说明该驱动没有独立 schema 层：此时**不产 schema 行**，表直接挂上一级。
pub fn schema_rows(conn: &str, catalog: &str, schemas: &[String], depth: u8) -> Vec<Row> {
    schemas
        .iter()
        .map(|s| {
            Row::new(
                NodeId::DbSchema {
                    conn: conn.to_string(),
                    catalog: catalog.to_string(),
                    schema: s.clone(),
                },
                depth,
                NodeKind::Folder,
                s.clone(),
            )
        })
        .collect()
}

/// 表行。`schema` 非空时标签折成 `schema · 表`（同名表才能区分），空则不折。
///
/// `schema` 参数只对「整组表同属一个 schema」的情形有意义（schema 行展开后的那一层）；
/// 多 catalog 时每张表自带 schema，逐行取自己的。
pub fn table_rows(
    conn: &str,
    catalog: &str,
    fallback_schema: &str,
    tables: &[TableCandidate],
    depth: u8,
) -> Vec<Row> {
    tables
        .iter()
        .map(|t| {
            let schema = if t.schema.is_empty() {
                fallback_schema
            } else {
                t.schema.as_str()
            };
            let label = if schema.is_empty() {
                t.name.clone()
            } else {
                format!("{} · {}", schema, t.name)
            };
            Row::new(
                NodeId::DbTable {
                    conn: conn.to_string(),
                    catalog: catalog.to_string(),
                    schema: schema.to_string(),
                    table: t.name.clone(),
                },
                depth,
                NodeKind::Table,
                label,
            )
        })
        .collect()
}

/// 数据库根的**已加载部分** → 行列表（未展开的节点只有自己那一行）。
///
/// 交给 `TreeState::visible_rows` 再按折叠态过滤：展开态是视图的事，缓存是数据的事。
pub fn db_rows(briefs: &[ConnBrief<'_>], cache: &DbCache) -> Vec<Row> {
    let mut out = Vec::new();
    for brief in briefs {
        out.push(conn_row(brief));
        append_kids(&NodeId::Conn(brief.id.to_string()), cache, &mut out);
    }
    out
}

fn append_kids(id: &NodeId, cache: &DbCache, out: &mut Vec<Row>) {
    let Some(kids) = cache.children(id) else {
        return;
    };
    for kid in kids {
        out.push(kid.clone());
        append_kids(&kid.id, cache, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::source_picker::PICKER_MAX_DEPTH;

    fn brief<'a>(id: &'a str, name: &'a str) -> ConnBrief<'a> {
        ConnBrief {
            id,
            name,
            driver: "PostgreSQL",
            host: "10.0.0.8",
        }
    }

    fn table(schema: &str, name: &str) -> TableCandidate {
        TableCandidate {
            name: name.to_string(),
            catalog: "main".to_string(),
            schema: schema.to_string(),
        }
    }

    #[test]
    fn conn_row_carries_driver_and_host_as_meta() {
        let row = conn_row(&brief("c1", "本地 PG"));
        assert_eq!(row.label, "本地 PG");
        assert_eq!(row.depth, 1, "连接固定在根下面第一层");
        assert_eq!(row.kind, NodeKind::Conn);
        assert_eq!(row.meta.as_deref(), Some("PostgreSQL · 10.0.0.8"));
        assert!(!row.is_confirmable(), "连接不是分析对象");
    }

    #[test]
    fn single_catalog_is_skipped_not_rendered() {
        let level = catalog_level("c1", &["analytics".to_string()], 2);
        assert_eq!(level, CatalogLevel::Single("analytics".to_string()));
        if let CatalogLevel::Many(rows) = level {
            assert!(rows.is_empty(), "单 catalog 不得产行");
        }
    }

    #[test]
    fn multiple_catalogs_each_become_a_row() {
        let level = catalog_level("c1", &["a".into(), "b".into()], 2);
        let CatalogLevel::Many(rows) = level else {
            panic!("多 catalog 必须成行");
        };
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].depth, 2);
        assert_eq!(rows[0].kind, NodeKind::Folder);
        assert!(rows[0].id.key().starts_with("db-catalog/c1/a"));
    }

    #[test]
    fn empty_catalog_list_yields_none() {
        assert_eq!(catalog_level("c1", &[], 2), CatalogLevel::None);
    }

    #[test]
    fn schema_rows_sit_at_depth_two() {
        let rows = schema_rows("c1", "analytics", &["public".into(), "sales".into()], 2);
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().all(|r| r.depth == 2 && r.kind == NodeKind::Folder));
    }

    #[test]
    fn empty_schema_list_means_tables_hang_one_level_up() {
        let rows = schema_rows("c1", "analytics", &[], 2);
        assert!(rows.is_empty(), "无独立 schema 层时不产 schema 行");
    }

    #[test]
    fn table_label_folds_schema_so_namesakes_stay_apart() {
        let rows = table_rows(
            "c1",
            "analytics",
            "sales",
            &[table("sales", "orders"), table("public", "orders")],
            3,
        );
        let labels: Vec<&str> = rows.iter().map(|r| r.label.as_str()).collect();
        assert_eq!(labels, vec!["sales · orders", "public · orders"]);
        assert_ne!(rows[0].id, rows[1].id, "同名表分属两个 schema，身份不能相同");
        assert!(rows.iter().all(|r| r.depth == 3 && r.is_confirmable()));
    }

    #[test]
    fn table_label_stays_plain_without_a_schema_layer() {
        let rows = table_rows("c1", "analytics", "", &[table("", "orders")], 2);
        assert_eq!(rows[0].label, "orders", "无 schema 层时不加分隔符");
        assert_eq!(rows[0].depth, 2);
        assert_eq!(
            rows[0].id,
            NodeId::DbTable {
                conn: "c1".into(),
                catalog: "analytics".into(),
                schema: "".into(),
                table: "orders".into(),
            }
        );
    }

    #[test]
    fn db_rows_is_flat_and_in_depth_first_order() {
        let mut cache = DbCache::new();
        cache.put(
            NodeId::Conn("c1".into()),
            schema_rows("c1", "analytics", &["sales".into()], 2),
        );
        cache.put(
            NodeId::DbSchema {
                conn: "c1".into(),
                catalog: "analytics".into(),
                schema: "sales".into(),
            },
            table_rows(
                "c1",
                "analytics",
                "sales",
                &[table("sales", "orders")],
                3,
            ),
        );
        let rows = db_rows(&[brief("c1", "本地 PG")], &cache);
        let labels: Vec<&str> = rows.iter().map(|r| r.label.as_str()).collect();
        assert_eq!(labels, vec!["本地 PG", "sales", "sales · orders"]);
        assert!(rows.iter().all(|r| r.depth <= PICKER_MAX_DEPTH));
    }

    #[test]
    fn unloaded_layers_contribute_no_rows() {
        let cache = DbCache::new();
        let rows = db_rows(&[brief("c1", "本地 PG")], &cache);
        assert_eq!(rows.len(), 1, "没展开的连接只有自己那一行");
        assert!(!cache.is_loaded(&NodeId::Conn("c1".into())));
    }

    #[test]
    fn empty_layer_is_marked_loaded_so_it_is_not_requeried() {
        let mut cache = DbCache::new();
        cache.put(NodeId::Conn("c1".into()), Vec::new());
        assert!(cache.is_loaded(&NodeId::Conn("c1".into())), "空结果也要记");
    }

    #[test]
    fn clearing_throws_away_every_layer() {
        let mut cache = DbCache::new();
        cache.put(NodeId::Conn("c1".into()), Vec::new());
        cache.clear();
        assert!(!cache.is_loaded(&NodeId::Conn("c1".into())));
    }
}

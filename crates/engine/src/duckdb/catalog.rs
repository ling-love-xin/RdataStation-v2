//! catalog 的**事实面**：一个别名一条，不是一次挂载一条。
//!
//! 设计约束（见 `docs/architecture/federation/catalog-context-model.md`）：
//!
//! 1. ATTACH 的结果归 DuckDB 所有，我们**不维护第二份挂载状态**；这里只做
//!    「把分散在各处的挂载事实归一读出来」，供面板 / 预检 / 诊断共用。
//! 2. 唯一我们自己存的东西是「别名 → 库文件路径」（[`super::file_view::catalog_files_snapshot`]），
//!    因为 `duckdb_databases()` 只说别名、说不出源路径。它的写入口只有 `load_database_global`
//!    一族与 `detach_database_global` 两个方向。
//! 3. 一个别名被多条会话持有是正常情况：`holders` 记齐，谁都不覆盖谁。
//!
//! 这一层不做任何过滤（不排除加速会话、不判"算不算分析会话"）——过滤是视图的事，
//! 快照只回答"此刻实例上有什么、谁在用"。

use std::collections::BTreeMap;

/// 一条 catalog 从哪来。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CatalogOrigin {
    /// 网络源（mysql / postgres / …），`conn_id` 是宿主那边的连接身份
    Network { conn_id: String, driver: String },
    /// 本机数据库文件（`.duckdb` / `.sqlite`）ATTACH 成的 catalog
    File { path: String },
    /// 分析底座（项目分析库 / 全局分析库），跟着会话走，不是用户加的源
    Base { path: String },
}

impl CatalogOrigin {
    /// 驱动口径的短名（界面 / 诊断用，不参与判定）
    pub fn driver(&self) -> &str {
        match self {
            Self::Network { driver, .. } => driver,
            Self::File { .. } => "file",
            Self::Base { .. } => "base",
        }
    }
}

/// 谁在用它。会话按 owner 记（文档 id / `data_objects` 之类），加速档按连接 id 记。
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum CatalogHolder {
    Session(String),
    Conn(String),
}

/// 一条 catalog 此刻的状态（与挂载时记下的原话一致，不翻译）
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CatalogState {
    Ready { tables: usize },
    Failed(String),
}

/// 归一后的一条 catalog
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogInfo {
    pub alias: String,
    pub origin: CatalogOrigin,
    pub state: CatalogState,
    /// 持有者清单（排序稳定：同一份输入必须给出同一份输出）
    pub holders: Vec<CatalogHolder>,
}

impl CatalogInfo {
    pub fn is_ready(&self) -> bool {
        matches!(self.state, CatalogState::Ready { .. })
    }

    /// 是不是用户加的源（底座不算）
    pub fn is_source(&self) -> bool {
        !matches!(self.origin, CatalogOrigin::Base { .. })
    }
}

/// 此刻实例上的全部 catalog（一个别名一条）。
///
/// 纯内存读、不拿连接锁、不做 I/O：渲染路径与预检都可以直接调。
/// 没有实例时只返回文件索引里的条目（那些是"意图已落地、实例还没起"的情形，
/// 属于诚实：不伪造可用）。
pub fn snapshot() -> Vec<CatalogInfo> {
    let mut by_alias: BTreeMap<String, CatalogInfo> = BTreeMap::new();

    if let Some(instance) = super::instance::current() {
        for info in instance.sessions() {
            let Some(session) = instance.session(&info.id) else {
                continue;
            };
            for mounted in session.mounted_sources() {
                let state = match &mounted.state {
                    super::federation::registry::MountState::Ready { tables } => CatalogState::Ready {
                        tables: *tables,
                    },
                    super::federation::registry::MountState::Failed(reason) => {
                        CatalogState::Failed(reason.clone())
                    }
                };
                let origin = CatalogOrigin::Network {
                    conn_id: mounted.source.conn_id.clone(),
                    driver: mounted.source.kind.label().to_string(),
                };
                merge(
                    &mut by_alias,
                    mounted.alias().to_string(),
                    origin,
                    state,
                    Some(CatalogHolder::Session(info.owner.clone())),
                );
            }
        }
        for (conn_id, alias, kind) in instance.mounted_conns() {
            let origin = CatalogOrigin::Network {
                conn_id: conn_id.clone(),
                driver: kind.label().to_string(),
            };
            merge(
                &mut by_alias,
                alias,
                origin,
                CatalogState::Ready { tables: 0 },
                Some(CatalogHolder::Conn(conn_id)),
            );
        }
    }

    for (alias, path) in super::file_view::catalog_files_snapshot() {
        merge(
            &mut by_alias,
            alias,
            CatalogOrigin::File { path },
            CatalogState::Ready { tables: 0 },
            None,
        );
    }

    for base in super::federation::session::bases() {
        merge(
            &mut by_alias,
            base.alias,
            CatalogOrigin::Base { path: base.path },
            CatalogState::Ready { tables: 0 },
            None,
        );
    }

    by_alias.into_values().collect()
}

/// 此刻每个 catalog 的表 / 视图名（一次读全，供预检回答「命中几个源」）。
///
/// 没有实例 → 空表：预检只校验别名，不猜表存在性（宁可漏报，不误报）。
/// 不缓存：这一层是 DuckDB 的派生读，任何缓存都得自己维护失效，反而成了第二份事实。
pub fn table_names() -> BTreeMap<String, Vec<String>> {
    let Some(instance) = super::instance::current() else {
        return BTreeMap::new();
    };
    let Ok(rows) = instance.catalog_table_names() else {
        return BTreeMap::new();
    };
    let mut map: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (alias, name) in rows {
        map.entry(alias).or_default().push(name);
    }
    for names in map.values_mut() {
        names.sort();
        names.dedup();
    }
    map
}

/// 同一个别名被多处描述时并成一行：持有者取并集，状态**先到的说了算**。
///
/// 先到优先是因为两类事实的可靠性不同：会话里记的是挂载那一刻的真实结果
/// （含失败原话），加速档与文件索引只记"挂上了"；若后两者覆盖前者，
/// 一条挂失败的源会被静默说成可用。来源（origin）同理，先到者保留。
fn merge(
    map: &mut BTreeMap<String, CatalogInfo>,
    alias: String,
    origin: CatalogOrigin,
    state: CatalogState,
    holder: Option<CatalogHolder>,
) {
    match map.get_mut(&alias) {
        Some(existing) => {
            if let Some(holder) = holder {
                if !existing.holders.contains(&holder) {
                    existing.holders.push(holder);
                    existing.holders.sort();
                }
            }
        }
        None => {
            map.insert(
                alias.clone(),
                CatalogInfo {
                    alias,
                    origin,
                    state,
                    holders: holder.into_iter().collect(),
                },
            );
        }
    }
}

#[cfg(test)]
mod tests {
    // 安全模式：精确导入，不通配
    use super::{merge, CatalogHolder, CatalogInfo, CatalogOrigin, CatalogState};
    use std::collections::BTreeMap;

    fn network(conn_id: &str) -> CatalogOrigin {
        CatalogOrigin::Network {
            conn_id: conn_id.to_string(),
            driver: "mysql".to_string(),
        }
    }

    /// 同一个别名被两条会话持有 → 只出一行，持有者记齐（这就是"一个源一条记录"的雏形）
    #[test]
    fn one_alias_stays_one_row_even_with_several_holders() {
        let mut map: BTreeMap<String, CatalogInfo> = BTreeMap::new();
        merge(
            &mut map,
            "shop".to_string(),
            network("C1"),
            CatalogState::Ready { tables: 3 },
            Some(CatalogHolder::Session("doc-a".to_string())),
        );
        merge(
            &mut map,
            "shop".to_string(),
            network("C1"),
            CatalogState::Ready { tables: 3 },
            Some(CatalogHolder::Session("doc-b".to_string())),
        );
        merge(
            &mut map,
            "shop".to_string(),
            network("C1"),
            CatalogState::Ready { tables: 3 },
            Some(CatalogHolder::Conn("C1".to_string())),
        );

        assert_eq!(map.len(), 1, "同一别名不得出两行");
        let row = map.get("shop").expect("应有这一行");
        assert_eq!(
            row.holders,
            vec![
                CatalogHolder::Session("doc-a".to_string()),
                CatalogHolder::Session("doc-b".to_string()),
                CatalogHolder::Conn("C1".to_string())
            ],
            "持有者要按稳定序排好（枚举声明序在前，字符串序在后）"
        );
    }

    /// 失败原话不能被后来的"挂上了"盖掉：先到者优先
    #[test]
    fn a_failure_is_not_silenced_by_a_later_success() {
        let mut map: BTreeMap<String, CatalogInfo> = BTreeMap::new();
        merge(
            &mut map,
            "orders".to_string(),
            network("C9"),
            CatalogState::Failed("扩展 mysql_scanner 未安装".to_string()),
            Some(CatalogHolder::Session("doc-a".to_string())),
        );
        merge(
            &mut map,
            "orders".to_string(),
            network("C9"),
            CatalogState::Ready { tables: 0 },
            Some(CatalogHolder::Conn("C9".to_string())),
        );

        let row = map.get("orders").expect("应有这一行");
        assert!(
            matches!(row.state, CatalogState::Failed(_)),
            "失败态被后来的 Ready 覆盖了，等于把挂载失败悄悄说成可用"
        );
        assert!(!row.is_ready());
    }

    /// 底座不算"用户加的源"
    #[test]
    fn a_base_catalog_is_not_a_source() {
        let mut map: BTreeMap<String, CatalogInfo> = BTreeMap::new();
        merge(
            &mut map,
            "analytics".to_string(),
            CatalogOrigin::Base {
                path: "D:/proj/.RSmeta/analytics.duckdb".to_string(),
            },
            CatalogState::Ready { tables: 0 },
            None,
        );
        let row = map.get("analytics").expect("应有这一行");
        assert!(!row.is_source());
    }
}

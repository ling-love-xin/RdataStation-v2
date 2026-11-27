//! 发送前预检：把「这条 SQL 引用的表在这个实例上到底指哪儿」在执行之前问清楚。
//!
//! 口径（与 `docs/architecture/federation/catalog-context-model.md` §4 一致）：
//!
//! 1. **不改写 SQL**。预检只读名字、只报错，绝不把裸名补成别名（CTE / 子查询 / 动态 SQL
//!    都会被文本重写弄坏）。
//! 2. 未限定名按 search_path 解析：主源命中 → 就是它（不报歧义）；主源没命中而**多个源**
//!    都有 → 报歧义并给候选；一个都没有 → 报不存在（**例外**：临时区列不出来时放行，
//!    见 `Facts::local_tables_visible`——临时表是连接级的，预检的连接看不到执行连接的
//!    临时表，看不见不等于不存在）。
//! 3. 限定名：首段是已知别名时校验别名与其表；不是别名且只有两段 → 当 `schema.table`
//!    放行（`main` / `rds_tmp` / 用户 schema，实例上没法在本地分清）；三段以上且首段
//!    不是别名 → 报 catalog 不存在。
//! 4. 事实不足就**不猜**：没有实例 / 列不出表时只保证「能解析」，不报存在性——
//!    宁可让 DuckDB 在执行时报原话，也不在预检里编一个错出来。
//!
//! 内置库（`memory` / `temp` / `system`）也进事实表（临时表就在 `memory` 里），但**不**
//! 参与「多源歧义」计数：用户在源之间选，不该被 DuckDB 自己的临时区搅进来。
//!
//! 错误只有一个出口：这里的 `Err(String)` 就是执行前闸门的拒绝理由，UI 直接展示。

use engine::duckdb::catalog::{self, CatalogState};

/// 一个别名的事实：可用不可用 + 它此刻有哪些表 / 视图。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogFact {
    pub alias: String,
    pub tables: Vec<String>,
    /// `Some(原话)` = 这个别名挂着但不可用（ATTACH 失败 / 扩展缺失），原话就是原因。
    pub unavailable: Option<String>,
    /// DuckDB 自带的库（`memory` / `temp` / `system`）：能解析，但不是「用户挂的源」。
    pub builtin: bool,
}

impl CatalogFact {
    fn has_table(&self, name: &str) -> bool {
        self.tables.iter().any(|t| t.eq_ignore_ascii_case(name))
    }
}

/// 预检看到的世界：实例上的全部别名 + 本文档的主源。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Facts {
    pub catalogs: Vec<CatalogFact>,
    /// search_path 的第一个（`USE` 的那个）。`None` = 没有主源，裸名靠后面的兜底。
    pub primary: Option<String>,
}

impl Facts {
    /// 从实例读一次事实（不缓存：这是 DuckDB 的派生读，缓存要自己维护失效，反成第二份真相）。
    pub fn capture(primary: Option<String>) -> Self {
        let names = catalog::table_names();
        let mut catalogs: Vec<CatalogFact> = catalog::snapshot()
            .into_iter()
            .map(|info| {
                let unavailable = match &info.state {
                    CatalogState::Failed(reason) => Some(reason.clone()),
                    CatalogState::Ready { .. } => None,
                };
                CatalogFact {
                    alias: info.alias.clone(),
                    // `duckdb_tables()` 的 `database_name` 与别名同形，但大小写不保证一致
                    tables: names
                        .iter()
                        .find(|(alias, _)| alias.eq_ignore_ascii_case(&info.alias))
                        .map(|(_, tables)| tables.clone())
                        .unwrap_or_default(),
                    unavailable,
                    builtin: false,
                }
            })
            .collect();
        // 内置库：`snapshot()` 里没有它们（那不是挂载事实），但临时表 / 系统表能按裸名查到。
        for builtin in BUILTIN_CATALOGS {
            if catalogs.iter().any(|c| c.alias.eq_ignore_ascii_case(builtin)) {
                continue;
            }
            let tables = names
                .iter()
                .find(|(alias, _)| alias.eq_ignore_ascii_case(builtin))
                .map(|(_, tables)| tables.clone())
                .unwrap_or_default();
            catalogs.push(CatalogFact {
                alias: (*builtin).to_string(),
                tables,
                unavailable: None,
                builtin: true,
            });
        }
        catalogs.sort_by(|a, b| a.alias.cmp(&b.alias));
        Self { catalogs, primary }
    }

    /// 没有实例时的空世界：预检退化为「只验能不能解析」。
    pub fn empty() -> Self {
        Self::default()
    }

    /// 报错文案里给用户看的别名（内置库不算「现有别名」）
    fn user_aliases(&self) -> Vec<&str> {
        self.catalogs
            .iter()
            .filter(|c| !c.builtin)
            .map(|c| c.alias.as_str())
            .collect()
    }

    /// 本机临时区（`temp` / `memory`）此刻**列不列得出来**。
    ///
    /// 临时表是连接级的：执行连接建的表，`control` 连接上的 `duckdb_tables()` 看不到。
    /// 所以「临时区里没有这张表」有两种含义——**真的没有**，或者**根本看不见**。
    /// 按本模块 §4「事实不足就不猜」：看不见时不能断言表不存在，交给 DuckDB 执行时报原话。
    fn local_tables_visible(&self) -> bool {
        self.catalogs
            .iter()
            .filter(|c| c.builtin && is_local_catalog(&c.alias))
            .any(|c| !c.tables.is_empty())
    }
}

/// DuckDB 自带的 catalog（与 `is_builtin_catalog` 同源，避免两处各写一份）
const BUILTIN_CATALOGS: [&str; 3] = ["memory", "temp", "system"];

/// 本机临时区：会话内的临时表落在这里（执行连接的 `temp` / `memory`）。
fn is_local_catalog(name: &str) -> bool {
    name.eq_ignore_ascii_case("temp") || name.eq_ignore_ascii_case("memory")
}

/// 本文档那条分析会话的主源（`None` = 没有主源 / 实例还没起）。
///
/// 预检的主源必须来自**同一份会话事实**：联邦档手里是 `FederatedSession`（传输层），
/// 加速档手里是 `AnalysisSession`，两边都按 owner（文档 id）回查实例才不会各说一套。
pub fn primary_of(owner: &str) -> Option<String> {
    engine::duckdb::instance::current()
        .and_then(|instance| instance.session_for_owner(owner))
        .and_then(|session| session.primary_alias())
}

/// 预检入口：`Ok` = 可以发；`Err` = 一句话拒绝理由（直接给用户看）。
pub fn check(sql: &str, dialect: engine::sql::SqlDialect, facts: &Facts) -> Result<(), String> {
    let paths = engine::sql::table_paths(sql, dialect)?;
    if facts.catalogs.is_empty() {
        return Ok(());
    }
    for path in paths {
        match path.depth() {
            0 => {}
            1 => check_bare(path.last(), facts)?,
            _ => check_qualified(&path, facts)?,
        }
    }
    Ok(())
}

/// 裸名：主源优先（search_path 的第一位就是它），没有主源才按命中数判歧义。
fn check_bare(name: &str, facts: &Facts) -> Result<(), String> {
    let hits: Vec<&CatalogFact> = facts
        .catalogs
        .iter()
        .filter(|c| c.unavailable.is_none() && c.has_table(name))
        .collect();
    if hits.is_empty() {
        // 临时区看不见（连接级表只在执行连接里）：不能因为「我没看到」就说表不存在
        if !facts.local_tables_visible() {
            return Ok(());
        }
        return Err(format!(
            "表「{name}」不存在：已挂载的源里都没有这张表（现有别名：{}）",
            facts.user_aliases().join("、")
        ));
    }
    // 内置库不参与歧义计数：临时表命中 memory 不该被当成「两个源都有」
    let sources: Vec<&CatalogFact> = hits.iter().copied().filter(|c| !c.builtin).collect();
    if sources.len() > 1 {
        let primary_hits = facts
            .primary
            .as_deref()
            .map(|primary| sources.iter().any(|c| c.alias.eq_ignore_ascii_case(primary)))
            .unwrap_or(false);
        if !primary_hits {
            let candidates: Vec<String> = sources
                .iter()
                .map(|c| format!("{}.{}", c.alias, name))
                .collect();
            return Err(format!(
                "表「{name}」在 {} 个源里都有：{}；执行时会按 search_path 落到其中一个，请加前缀指明",
                sources.len(),
                candidates.join(" / ")
            ));
        }
    }
    Ok(())
}

/// 限定名：首段是别名 → 校验别名与表；首段不是别名 → 两段当 schema 放行，三段报 catalog 不存在。
fn check_qualified(path: &engine::sql::TablePath, facts: &Facts) -> Result<(), String> {
    let head = &path.parts[0];
    let Some(fact) = facts
        .catalogs
        .iter()
        .find(|c| c.alias.eq_ignore_ascii_case(head))
    else {
        if path.depth() == 2 || is_builtin_catalog(head) {
            return Ok(());
        }
        return Err(format!(
            "catalog「{head}」不存在：现有别名 {}",
            facts.user_aliases().join("、")
        ));
    };
    if let Some(reason) = &fact.unavailable {
        return Err(format!("源「{}」当前不可用：{reason}", fact.alias));
    }
    if !fact.has_table(path.last()) {
        return Err(format!("源「{}」里没有表「{}」", fact.alias, path.last()));
    }
    Ok(())
}

/// DuckDB 自带 catalog：引用它们不该报「catalog 不存在」。
fn is_builtin_catalog(name: &str) -> bool {
    BUILTIN_CATALOGS
        .iter()
        .any(|builtin| name.eq_ignore_ascii_case(builtin))
        || name.eq_ignore_ascii_case("information_schema")
}

#[cfg(test)]
mod tests {
    // 安全模式：精确导入，不通配
    use super::{check, CatalogFact, Facts};
    use engine::sql::SqlDialect;

    fn fact(alias: &str, tables: &[&str]) -> CatalogFact {
        CatalogFact {
            alias: alias.to_string(),
            tables: tables.iter().map(|t| t.to_string()).collect(),
            unavailable: None,
            builtin: false,
        }
    }

    fn builtin(alias: &str, tables: &[&str]) -> CatalogFact {
        CatalogFact {
            builtin: true,
            ..fact(alias, tables)
        }
    }

    fn facts(primary: Option<&str>, catalogs: Vec<CatalogFact>) -> Facts {
        Facts {
            catalogs,
            primary: primary.map(str::to_string),
        }
    }

    fn duck(sql: &str, world: &Facts) -> Result<(), String> {
        check(sql, SqlDialect::Duckdb, world)
    }

    #[test]
    fn single_source_bare_name_resolves() {
        let world = facts(Some("shop"), vec![fact("shop", &["orders"])]);
        assert!(duck("SELECT * FROM orders", &world).is_ok());
    }

    #[test]
    fn missing_bare_name_names_the_aliases() {
        // 临时区看得见（非空）→ 才能断言「哪里都没有这张表」
        let world = facts(
            Some("shop"),
            vec![fact("shop", &["orders"]), builtin("temp", &["tmp_q_1"])],
        );
        let error = duck("SELECT * FROM nope", &world).unwrap_err();
        assert!(error.contains("nope"));
        assert!(error.contains("shop"), "报错要给出可用的别名：{error}");
    }

    #[test]
    fn missing_bare_name_is_let_through_when_local_area_is_invisible() {
        // 临时表是连接级的：`control` 连接列不到执行连接的 `temp`（真机实证：
        // `CREATE TEMP TABLE` 成功、`duckdb_tables()` 里也有 `temp.<name>`，但预检看不见）。
        // 看不见 ≠ 不存在，按 §4 放行，让 DuckDB 执行时报原话。
        let world = facts(
            Some("shop"),
            vec![fact("shop", &["orders"]), builtin("temp", &[])],
        );
        assert!(
            duck("SELECT * FROM rds_accel_vars", &world).is_ok(),
            "列不到临时区时不得把合法临时表拦成「不存在」"
        );
    }

    #[test]
    fn two_sources_and_no_primary_is_ambiguous() {
        let world = facts(None, vec![fact("shop", &["orders"]), fact("erp", &["orders"])]);
        let error = duck("SELECT * FROM orders", &world).unwrap_err();
        assert!(error.contains("shop.orders"), "要给出候选：{error}");
        assert!(error.contains("erp.orders"), "要给出候选：{error}");
    }

    #[test]
    fn primary_wins_over_ambiguity() {
        let world = facts(
            Some("shop"),
            vec![fact("shop", &["orders"]), fact("erp", &["orders"])],
        );
        assert!(
            duck("SELECT * FROM orders", &world).is_ok(),
            "主源就是 search_path 的第一位，裸名落到它不算歧义"
        );
    }

    #[test]
    fn alias_qualified_table_is_checked() {
        let world = facts(Some("shop"), vec![fact("shop", &["orders"])]);
        assert!(duck("SELECT * FROM shop.orders", &world).is_ok());
        let error = duck("SELECT * FROM shop.nope", &world).unwrap_err();
        assert!(error.contains("shop"));
        assert!(error.contains("nope"));
    }

    #[test]
    fn three_part_name_checks_catalog_and_table() {
        let world = facts(Some("shop"), vec![fact("shop", &["orders"])]);
        assert!(duck("SELECT * FROM shop.main.orders", &world).is_ok());
        let error = duck("SELECT * FROM ghost.main.orders", &world).unwrap_err();
        assert!(error.contains("ghost"), "未知 catalog 要点名：{error}");
    }

    #[test]
    fn two_part_name_with_schema_head_is_let_through() {
        let world = facts(Some("shop"), vec![fact("shop", &["orders"])]);
        assert!(
            duck("SELECT * FROM main.orders", &world).is_ok(),
            "两段首段不是别名 = schema.table，本地分不清，不在预检里判"
        );
    }

    #[test]
    fn builtin_catalogs_are_not_reported_as_missing() {
        let world = facts(Some("shop"), vec![fact("shop", &["orders"])]);
        assert!(duck("SELECT * FROM memory.rds_tmp.tmp_q_1", &world).is_ok());
    }

    #[test]
    fn temp_table_in_memory_resolves_and_does_not_create_ambiguity() {
        let world = facts(
            Some("shop"),
            vec![
                fact("shop", &["orders"]),
                builtin("memory", &["tmp_q_1", "orders"]),
            ],
        );
        assert!(
            duck("SELECT * FROM tmp_q_1", &world).is_ok(),
            "临时表在 memory 里，裸名能解析；它也不该和源里的表构成歧义"
        );
        assert!(
            duck("SELECT * FROM orders", &world).is_ok(),
            "memory 命中不算第二个源（内置库不参与歧义）"
        );
    }

    #[test]
    fn unavailable_source_fails_with_its_own_reason() {
        let world = facts(
            Some("shop"),
            vec![
                fact("shop", &["orders"]),
                CatalogFact {
                    alias: "erp".to_string(),
                    tables: Vec::new(),
                    unavailable: Some("扩展 mysql_scanner 未安装".to_string()),
                    builtin: false,
                },
            ],
        );
        let error = duck("SELECT * FROM erp.orders", &world).unwrap_err();
        assert!(error.contains("erp"));
        assert!(error.contains("mysql_scanner"), "原话要透传：{error}");
    }

    #[test]
    fn cte_and_derived_table_are_not_reported() {
        let world = facts(Some("shop"), vec![fact("shop", &["orders"])]);
        assert!(duck("WITH c AS (SELECT id FROM orders) SELECT * FROM c", &world).is_ok());
        assert!(duck(
            "SELECT * FROM (SELECT id FROM orders) x JOIN orders o ON x.id = o.id",
            &world
        )
        .is_ok());
    }

    #[test]
    fn empty_world_only_checks_parsing() {
        assert!(duck("SELECT * FROM anything", &Facts::empty()).is_ok());
        assert!(check("SELECT FROM", SqlDialect::Duckdb, &Facts::empty()).is_err());
    }

    #[test]
    fn union_branches_are_all_checked() {
        let world = facts(Some("shop"), vec![fact("shop", &["orders"])]);
        let error = duck("SELECT * FROM orders UNION ALL SELECT * FROM shop.nope", &world).unwrap_err();
        assert!(error.contains("nope"));
    }
}

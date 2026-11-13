//! 元数据面能力门控（D0 第二块：门控输入从「族」改「驱动级」）。
//!
//! ## 为什么不是族
//!
//! 同一族里两个实现的元数据面**不一样**（实测 2026-11-12）：
//! * MySQL 族：`mysql`(sqlx) 有索引明细与"触发器直查"，`mysql_native` 两者都没有；
//! * PG 族：两个实现都给不出源版 DDL（没有 `SHOW CREATE` 等价物）；
//! * `sqlite` 没覆写 `list_triggers`、`duckdb` 没覆写 `list_sequences`（真缺口）。
//!
//! 按族推会把这些差异抹平，表现就是「分区在、里面 0 条」——正是要消灭的静默降级。
//!
//! ## 输入是什么
//!
//! `drivers.capabilities` 里的 `meta_*` 键（声明权威在代码
//! `rdata…/crates/driver/src/registry/descriptors.rs`，启动由
//! `declaration.rs::sync_driver_declarations` 幂等 upsert 入库；插件驱动由
//! `plugin/src/driver_decl.rs` 落库）。：
//!
//! * 声明**有**该键 → 可用（拿到空集合 = 这张表真的没有，沿用 `(0)` 文案）；
//! * 声明**没有** → 置灰 + 原因（驱动给不出，不是"没有数据"）；
//! * 取不到声明（未知驱动 / 全局库未就绪）= `None` → 退回族默认（旧行为），
//!   宁可多显示也不误灰——"不知道"不等于"不支持"。
//!
//! 与 [`super::DriverCapabilities`]（`caps_for(db_type)`）的分工：那个管**列展示裁剪**
//! 与**类别是否存在**（族级 / 产品实现状态）；本模块管**这个驱动此刻能不能给出数据**
//! （驱动级 / 通道状态）。

/// 键名与 `crates/driver/src/capability.rs` 的字典一致（改名要两边同时改）。
pub const KEY_INDEX_DETAIL: &str = "meta_index_detail";
pub const KEY_CONSTRAINT_DETAIL: &str = "meta_constraint_detail";
pub const KEY_DDL_SOURCE: &str = "meta_ddl_source";
pub const KEY_DDL_COMPOSE: &str = "meta_ddl_compose";
pub const KEY_ROUTINE_SOURCE: &str = "meta_routine_source";

/// 单维度门控。原因文案直接给用户看：写"驱动给不出什么"，不写"暂无数据"。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gate {
    Supported,
    Unsupported(&'static str),
}

impl Gate {
    pub fn is_supported(self) -> bool {
        matches!(self, Self::Supported)
    }

    /// 不可用原因（可用时 `None`）。
    pub fn reason(self) -> Option<&'static str> {
        match self {
            Self::Supported => None,
            Self::Unsupported(r) => Some(r),
        }
    }

    fn from_declared(has_key: bool, reason: &'static str) -> Self {
        if has_key {
            Self::Supported
        } else {
            Self::Unsupported(reason)
        }
    }
}

const REASON_INDEX: &str = "该驱动未声明索引明细（meta_index_detail），没有这条内省通道";
const REASON_CONSTRAINT: &str =
    "该驱动未声明约束明细（meta_constraint_detail），没有这条内省通道";
const REASON_DDL_SOURCE: &str = "该驱动未声明源版 DDL（meta_ddl_source）：它没有 SHOW CREATE 等价物";
const REASON_DDL_COMPOSE: &str =
    "该驱动既无源版 DDL，也不声明可合成（meta_ddl_compose），DDL 无从产出";
const REASON_ROUTINE: &str =
    "该驱动未声明例程源码（meta_routine_source），拿不到 CREATE PROCEDURE/FUNCTION 原文";

/// 属性面板需要的五个门控维度。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MetadataGates {
    /// 「索引」分区
    pub index_detail: Gate,
    /// 「约束」分区
    pub constraint_detail: Gate,
    /// 「DDL（源版）」
    pub ddl_source: Gate,
    /// 「DDL（合成）」（产品能力：声明缺席的驱动连列/约束都拿不到，合成也无从下手）
    pub ddl_compose: Gate,
    /// 「源码」（例程 / 视图定义）
    pub routine_source: Gate,
}

impl MetadataGates {
    /// 全可用（未注册 provider 的旧路径 / 测试用；等价于"没接门控"）。
    pub fn all_supported() -> Self {
        Self {
            index_detail: Gate::Supported,
            constraint_detail: Gate::Supported,
            ddl_source: Gate::Supported,
            ddl_compose: Gate::Supported,
            routine_source: Gate::Supported,
        }
    }

    /// 由驱动声明的能力键算门控。
    ///
    /// `declared == None` 表示**取不到声明**（未知驱动 / 全局库没开 / 连接信息不全），
    /// 此时退回族默认而不是一律置灰——否则一次 DB 读取失败会把所有分区染灰。
    pub fn resolve(db_type: Option<&str>, declared: Option<&[String]>) -> Self {
        match declared {
            Some(keys) => Self::from_keys(keys),
            None => Self::family_default(db_type),
        }
    }

    /// 严格按声明：缺席即不可用（含"声明为空"——插件驱动还没实现元数据面时就是这个）。
    pub fn from_keys(keys: &[String]) -> Self {
        let has = |k: &str| keys.iter().any(|x| x == k);
        Self {
            index_detail: Gate::from_declared(has(KEY_INDEX_DETAIL), REASON_INDEX),
            constraint_detail: Gate::from_declared(has(KEY_CONSTRAINT_DETAIL), REASON_CONSTRAINT),
            ddl_source: Gate::from_declared(has(KEY_DDL_SOURCE), REASON_DDL_SOURCE),
            ddl_compose: Gate::from_declared(has(KEY_DDL_COMPOSE), REASON_DDL_COMPOSE),
            routine_source: Gate::from_declared(has(KEY_ROUTINE_SOURCE), REASON_ROUTINE),
        }
    }

    /// 族默认（仅用于"取不到声明"的兜底）：与 `caps_for(db_type)` 同一份认知。
    fn family_default(db_type: Option<&str>) -> Self {
        match db_type {
            // PG：没有 SHOW CREATE 等价物；其余全有。
            Some("postgresql") => Self {
                ddl_source: Gate::Unsupported(REASON_DDL_SOURCE),
                ..Self::all_supported()
            },
            // SQLite / DuckDB：存不下例程（存储过程/函数）。
            Some("sqlite") | Some("duckdb") => Self {
                routine_source: Gate::Unsupported(REASON_ROUTINE),
                ..Self::all_supported()
            },
            // MySQL 及未知族：都曾有实现（未知驱动按"不误灰"处理）。
            _ => Self::all_supported(),
        }
    }

    /// 置灰维度清单（测试 / 日志用：便于断言"这一族此刻灰哪几块"）。
    pub fn unsupported_dims(&self) -> Vec<&'static str> {
        [
            (KEY_INDEX_DETAIL, self.index_detail),
            (KEY_CONSTRAINT_DETAIL, self.constraint_detail),
            (KEY_DDL_SOURCE, self.ddl_source),
            (KEY_DDL_COMPOSE, self.ddl_compose),
            (KEY_ROUTINE_SOURCE, self.routine_source),
        ]
        .into_iter()
        .filter(|(_, g)| g.reason().is_some())
        .map(|(k, _)| k)
        .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    /// 有声明 → 可用；缺声明 → 置灰。PG 的「源版 DDL」就是这么灰的。
    #[test]
    fn declared_keys_decide_per_dimension() {
        let pg = keys(&[
            "tree",
            "meta_index_detail",
            "meta_constraint_detail",
            "meta_ddl_compose",
            "meta_routine_source",
        ]);
        let g = MetadataGates::from_keys(&pg);
        assert!(g.index_detail.is_supported());
        assert!(g.constraint_detail.is_supported());
        assert!(g.ddl_compose.is_supported());
        assert!(!g.ddl_source.is_supported(), "PG 缺源版 DDL");
        assert_eq!(
            g.ddl_source.reason().unwrap(),
            REASON_DDL_SOURCE,
            "原因要能直接展示给用户"
        );
        assert_eq!(g.unsupported_dims(), vec![KEY_DDL_SOURCE]);
    }

    /// 声明为空（插件驱动还没实现元数据面）→ 五个维度全灰，绝不假装有数据。
    #[test]
    fn empty_declaration_greys_everything() {
        let g = MetadataGates::from_keys(&[]);
        assert_eq!(g.unsupported_dims().len(), 5);
        assert!(!g.index_detail.is_supported());
        assert!(!g.ddl_compose.is_supported());
    }

    /// 取不到声明（None）→ 族默认：PG 灰源版 DDL，SQLite/DuckDB 灰例程源码，MySQL 全开。
    #[test]
    fn missing_declaration_falls_back_to_family_defaults() {
        let pg = MetadataGates::resolve(Some("postgresql"), None);
        assert!(!pg.ddl_source.is_supported());
        assert!(pg.index_detail.is_supported());

        let sq = MetadataGates::resolve(Some("sqlite"), None);
        assert!(!sq.routine_source.is_supported());
        assert!(sq.ddl_source.is_supported(), "SQLite 有 sqlite_master.sql");

        let my = MetadataGates::resolve(Some("mysql"), None);
        assert_eq!(my.unsupported_dims().len(), 0);

        let unknown = MetadataGates::resolve(Some("oracle"), None);
        assert!(unknown.index_detail.is_supported(), "未知驱动不误灰");
    }

    /// 原因必须非空且互相不同（渲染层只给一行灰字，不能五块用同一句）。
    #[test]
    fn reasons_are_distinct_and_non_empty() {
        let g = MetadataGates::from_keys(&[]);
        let reasons: Vec<&str> = [
            g.index_detail,
            g.constraint_detail,
            g.ddl_source,
            g.ddl_compose,
            g.routine_source,
        ]
        .into_iter()
        .filter_map(|x| x.reason())
        .collect();
        assert_eq!(reasons.len(), 5);
        for r in &reasons {
            assert!(!r.trim().is_empty());
        }
        let mut uniq = reasons.clone();
        uniq.sort_unstable();
        uniq.dedup();
        assert_eq!(uniq.len(), 5, "原因重复：{reasons:#?}");
    }
}

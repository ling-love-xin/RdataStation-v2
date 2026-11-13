//! 来源的「三分法」分组：根 3 / 叶 5。
//!
//! 根只回答一件事：**这份数据怎么读**（驱动 + 连接 / 本地 SQL / reader 函数），
//! 它决定执行路径、物化方式与错误类型；叶只表达「在哪、还能活多久」。
//!
//! 硬约束（`federation-entry-design.md` §4）：枚举保持 5 个叶子不变，
//! 分组只在视图层做纯函数运算——`PickerConfig.kinds` / `has_target()` /
//! `confirm_prefix` 都不因此改动。

use super::SourceKind;

/// 三个根。与「持久/临时 × 本机/外部」二维表里用户可感知的三格一一对应：
/// 数据库 = 外部·持久，DuckDB = 本机·临时，文件 = 本机·持久。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SourceGroup {
    /// 驱动 + 连接，SQL 下推；连接在，表就在
    Database,
    /// 本地 SQL，装的是临时产物（结果集 / Mock / 插件 / 会话表）
    DuckDb,
    /// reader 函数，装的是本机持久物（草稿 / 资产库 / 外部文件）
    File,
}

impl SourceGroup {
    /// 组标题（模态内 tab 文案）
    pub fn title(self) -> &'static str {
        match self {
            Self::Database => "数据库",
            Self::DuckDb => "DuckDB",
            Self::File => "文件",
        }
    }

    /// 组内一行说明：为什么这几类归到一起（回答「怎么读」，不回答「是什么」）
    pub fn note(self) -> &'static str {
        match self {
            Self::Database => "驱动 + 连接，SQL 下推；连接在，表就在",
            Self::DuckDb => "本地 SQL，装的是临时产物；随会话或单例存活",
            Self::File => "reader 函数读取，装的是本机持久物；靠文件/定义存活",
        }
    }
}

/// 叶子 → 根（纯函数，无状态）。
///
/// - `ResultSet` 并入 DuckDB 组（它本来就是引擎里的临时表，不单开 tab）；
/// - `Analysis` 与 `Scratchpad` / `External` 同属文件组：前两者的本体分别是
///   `{项目}/.RSmeta/analytics.duckdb` 与 `{项目}/resources/`，都是本机持久物。
pub fn group_of(kind: SourceKind) -> SourceGroup {
    match kind {
        SourceKind::Connected => SourceGroup::Database,
        SourceKind::ResultSet => SourceGroup::DuckDb,
        SourceKind::Analysis | SourceKind::Scratchpad | SourceKind::External => SourceGroup::File,
    }
}

/// 按 `PickerConfig.kinds` 的给定顺序聚成根：同根合并、不重排入参、空组不出现。
///
/// 返回值顺序 = 该根第一次出现的顺序；组内顺序 = 入参顺序。
/// 调用方据此渲染「三个根各自的表 / 视图」，而不必自己维护一遍分组表。
pub fn grouped(kinds: &[SourceKind]) -> Vec<(SourceGroup, Vec<SourceKind>)> {
    let mut out: Vec<(SourceGroup, Vec<SourceKind>)> = Vec::new();
    for &k in kinds {
        let g = group_of(k);
        match out.iter_mut().find(|(group, _)| *group == g) {
            Some((_, members)) => members.push(k),
            None => out.push((g, vec![k])),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [SourceKind; 5] = [
        SourceKind::Connected,
        SourceKind::Scratchpad,
        SourceKind::Analysis,
        SourceKind::External,
        SourceKind::ResultSet,
    ];

    #[test]
    fn every_kind_maps_to_exactly_one_group() {
        // 穷举 5 个叶子：新增叶子时这里会编译失败，逼着补映射
        for k in ALL {
            let g = group_of(k);
            assert!(matches!(
                g,
                SourceGroup::Database | SourceGroup::DuckDb | SourceGroup::File
            ));
        }
    }

    #[test]
    fn result_set_joins_duckdb_group() {
        // 裁决 #7：结果集并入 DuckDB 组，不单开 tab
        assert_eq!(group_of(SourceKind::ResultSet), SourceGroup::DuckDb);
    }

    #[test]
    fn analysis_joins_file_group() {
        // 裁决 #8：资产库（原「资源分析」）挂文件根，本体是本机持久物
        assert_eq!(group_of(SourceKind::Analysis), SourceGroup::File);
    }

    #[test]
    fn only_connected_is_database_root() {
        for k in ALL {
            assert_eq!(
                group_of(k) == SourceGroup::Database,
                k == SourceKind::Connected,
                "只有 Connected 属于数据库根，实际 {k:?}"
            );
        }
    }

    #[test]
    fn grouped_merges_same_root_and_keeps_input_order() {
        let kinds = [
            SourceKind::Scratchpad,
            SourceKind::Connected,
            SourceKind::External,
            SourceKind::ResultSet,
        ];
        let got = grouped(&kinds);
        // 顺序 = 第一次出现的顺序：File(草稿) → Database → DuckDb
        assert_eq!(got.len(), 3);
        assert_eq!(got[0].0, SourceGroup::File);
        assert_eq!(got[0].1, vec![SourceKind::Scratchpad, SourceKind::External]);
        assert_eq!(got[1].0, SourceGroup::Database);
        assert_eq!(got[1].1, vec![SourceKind::Connected]);
        assert_eq!(got[2].0, SourceGroup::DuckDb);
        assert_eq!(got[2].1, vec![SourceKind::ResultSet]);
    }

    #[test]
    fn grouped_omits_empty_groups() {
        // 洞察裁剪掉 ResultSet 时，DuckDB 组不应出现空标题
        let kinds = [SourceKind::Connected, SourceKind::Scratchpad];
        let got = grouped(&kinds);
        assert_eq!(got.len(), 2);
        assert!(got.iter().all(|(g, _)| *g != SourceGroup::DuckDb));
    }

    #[test]
    fn grouped_never_loses_or_duplicates_a_kind() {
        let kinds = ALL;
        let total: usize = grouped(&kinds).iter().map(|(_, m)| m.len()).sum();
        assert_eq!(total, kinds.len());
    }
}

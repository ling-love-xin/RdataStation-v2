//! 领域类型：种类与状态、请求与回执、变更事件。

mod affordance;
mod kind;
mod promoted_from;
mod semantics;
mod timeline;
mod types;

pub use affordance::*;
pub use kind::*;
pub use promoted_from::*;
pub use semantics::*;
pub use timeline::*;
pub use types::*;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_db_roundtrip() {
        for kind in [
            ArchiveKind::File,
            ArchiveKind::AnalyticsTable,
            ArchiveKind::TableRef,
        ] {
            assert_eq!(ArchiveKind::from_db_str(kind.as_db_str()), kind);
        }
        assert_eq!(ArchiveKind::default(), ArchiveKind::File);
        // 表型落库折回 `analysis`（020 的 CHECK 只认三档，见迁移 031 注释）。
        assert_eq!(ArchiveKind::AnalyticsTable.db_write_str(), "analysis");
        assert_eq!(ArchiveKind::File.db_write_str(), "file");
    }

    #[test]
    fn resolve_follows_analytics_table_column_not_kind_string() {
        // 表型行在库里 `kind` 仍是 `analysis`——只有 `analytics_table` 列能认出它。
        assert_eq!(
            ArchiveKind::resolve("analysis", Some("t_sales")),
            ArchiveKind::AnalyticsTable
        );
        // `analysis` 是历史串：它的本体本来就是受管文件，读成 `File`。
        assert_eq!(ArchiveKind::resolve("analysis", None), ArchiveKind::File);
        assert_eq!(ArchiveKind::resolve("analysis", Some("")), ArchiveKind::File);
        assert_eq!(ArchiveKind::resolve("file", None), ArchiveKind::File);
        assert_eq!(
            ArchiveKind::resolve("table_ref", None),
            ArchiveKind::TableRef
        );
    }

    #[test]
    fn kind_labels_cover_all_kinds_in_menu_order() {
        let labels: Vec<&str> = ArchiveKind::ALL.iter().map(|kind| kind.label()).collect();
        assert_eq!(labels, vec!["文件", "分析库表", "引用"]);
        // 筛选菜单只给两档：引用档没有新写入点，不进菜单（避免"看起来还能归档引用"）。
        let menu: Vec<&str> = ArchiveKind::MENU.iter().map(|kind| kind.label()).collect();
        assert_eq!(menu, vec!["文件", "分析库表"]);
    }

    #[test]
    fn kind_unknown_value_falls_back_to_file() {
        // 新种类由旧版本读到、或 007 时代旧行，都不应整表失败。
        assert_eq!(ArchiveKind::from_db_str("future_kind"), ArchiveKind::File);
        assert_eq!(ArchiveKind::from_db_str(""), ArchiveKind::File);
    }

    #[test]
    fn strength_is_derived_from_kind() {
        assert_eq!(ArchiveKind::File.strength(), ReproductionStrength::Strong);
        // 表型资产本体是本机库里的表：同样只能兑现「内容冻结」这一档。
        assert_eq!(
            ArchiveKind::AnalyticsTable.strength(),
            ReproductionStrength::Strong
        );
        assert_eq!(ArchiveKind::TableRef.strength(), ReproductionStrength::Weak);
        // 表型同样内容冻结在库内（表不会自己消失），与文件档同级。
        assert_eq!(
            ArchiveKind::AnalyticsTable.strength(),
            ReproductionStrength::Strong
        );
        assert_eq!(ArchiveKind::TableRef.strength(), ReproductionStrength::Weak);
    }

    #[test]
    fn binding_default_is_all_empty() {
        let binding = ArchiveBinding::default();
        assert!(binding.promoted_from.is_none());
        assert!(binding.source_connection_id.is_none());
        assert!(binding.source_table.is_none());
    }
}

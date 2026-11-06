//! 领域类型：种类与状态、请求与回执、变更事件。

mod kind;
mod types;

pub use kind::*;
pub use types::*;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_db_roundtrip() {
        for kind in [
            ArchiveKind::File,
            ArchiveKind::Analysis,
            ArchiveKind::TableRef,
        ] {
            assert_eq!(ArchiveKind::from_db_str(kind.as_db_str()), kind);
        }
        assert_eq!(ArchiveKind::default(), ArchiveKind::File);
    }

    #[test]
    fn kind_labels_cover_all_kinds_in_menu_order() {
        let labels: Vec<&str> = ArchiveKind::ALL.iter().map(|kind| kind.label()).collect();
        assert_eq!(labels, vec!["文件", "分析表", "引用"]);
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
        assert_eq!(
            ArchiveKind::Analysis.strength(),
            ReproductionStrength::Medium
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

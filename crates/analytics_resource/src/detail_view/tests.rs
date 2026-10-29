    // 安全模式：测试模块不通配导入。
    use super::{ArchiveDetail, ArchiveTagChip, alert_line, detail_rows, kind_label, short_hash};
    use crate::model::{ArchiveKind, ArchiveStatus, PayloadMode, RowScope};

    fn detail(status: ArchiveStatus, kind: ArchiveKind) -> ArchiveDetail {
        ArchiveDetail {
            id: "ar_1".to_string(),
            name: "dau_report".to_string(),
            alias: None,
            kind,
            version: 2,
            status,
            readonly: true,
            payload_mode: PayloadMode::Copy,
            source_path: None,
            scope: RowScope::Project,
            size_label: "1.2 KB".to_string(),
            modified_label: "3 天前".to_string(),
            archived_label: "3 天前".to_string(),
            promoted_from: Some("scratchpad/dau.sql".to_string()),
            source_connection_id: Some("conn_1".to_string()),
            source_table: None,
            content_hash: Some("0123456789abcdef0123".to_string()),
            payload_rel_path: Some("dau.sql".to_string()),
            history_label: "1 个历史版本".to_string(),
            tags: Vec::new(),
            group: None,
            // 预览缺省 = 仅元信息（各用例需要时用结构体更新语法盖掉）。
            preview: crate::preview::Preview::default(),
        }
    }

    #[test]
    fn short_hash_truncates_and_tolerates_missing() {
        assert_eq!(short_hash(Some("0123456789abcdef0123")), "0123456789ab");
        assert_eq!(short_hash(None), "—");
        assert_eq!(short_hash(Some("")), "—");
    }

    #[test]
    fn rows_skip_empty_values_but_keep_fingerprint_placeholder() {
        let rows = detail_rows(&detail(ArchiveStatus::Normal, ArchiveKind::File));
        let basic = rows
            .iter()
            .find(|(title, _)| title == "基本信息")
            .expect("基本信息");
        // 只读说明必须出现（用户最常问"为什么不能编辑"）。
        assert!(
            basic
                .1
                .iter()
                .any(|(label, value)| *label == "只读" && value.contains("取回"))
        );

        let source = rows
            .iter()
            .find(|(title, _)| title == "来源")
            .expect("来源");
        assert!(source.1.iter().any(|(label, _)| *label == "来源草稿"));
        // 没有来源表 → 不产生该行；指纹则恒有（无值给破折号）。
        assert!(!source.1.iter().any(|(label, _)| *label == "来源表"));
        assert!(source.1.iter().any(|(label, _)| *label == "内容指纹"));
    }

    #[test]
    fn organization_section_keeps_only_the_group() {
        let rows = detail_rows(&detail(ArchiveStatus::Normal, ArchiveKind::File));
        assert!(
            !rows.iter().any(|(title, _)| title == "组织"),
            "无分组时不应出现空分区"
        );

        let mut with_group = detail(ArchiveStatus::Normal, ArchiveKind::File);
        // 标签不再走（标签, 值）两列（chips 可交互，在 `render_tag_section` 里渲染）。
        with_group.tags = vec![ArchiveTagChip {
            id: "at_1".to_string(),
            name: "报表".to_string(),
        }];
        with_group.group = Some("报表".to_string());
        let rows = detail_rows(&with_group);
        let org = rows
            .iter()
            .find(|(title, _)| title == "组织")
            .expect("组织");
        assert!(
            org.1
                .iter()
                .any(|(label, value)| *label == "分组" && value == "报表")
        );
        assert!(
            !org.1.iter().any(|(label, _)| *label == "标签"),
            "标签不进文本行"
        );
    }

    #[test]
    fn alerts_only_for_actionable_states() {
        assert!(alert_line(&detail(ArchiveStatus::Normal, ArchiveKind::File)).is_none());
        assert!(
            alert_line(&detail(ArchiveStatus::ContentChanged, ArchiveKind::File))
                .expect("内容已变应提示")
                .contains("内容已变")
        );
        assert!(
            alert_line(&detail(ArchiveStatus::Missing, ArchiveKind::File))
                .expect("缺失应提示")
                .contains("本体缺失")
        );
        // 引用型常态就要提示（复现强度最弱，不等失效）。
        assert!(
            alert_line(&detail(ArchiveStatus::Normal, ArchiveKind::TableRef))
                .expect("引用应提示")
                .contains("源表可能已变更")
        );
    }

    #[test]
    fn kind_labels_are_shared_wording() {
        assert_eq!(kind_label(ArchiveKind::File), "受管文件");
        assert_eq!(kind_label(ArchiveKind::Analysis), "分析表");
        assert_eq!(kind_label(ArchiveKind::TableRef), "远端引用");
    }

    // 安全模式：测试模块不通配导入（会与 `#[gpui_kit::test]` 展开的 `#[test]` 自相残杀）。
    use super::{
        ArchiveCounts, ArchiveRow, BadgeTone, HeaderMenuAction, RowClick, apply_row_click,
        badge_tone, can_rename, can_view_stats, classify_click, default_group_entry_label,
        header_fold_key, kind_icon, row_tail, strength_badge, tag_entry_label, tag_targets,
    };
    use crate::detail_view::ArchiveDetail;
    use crate::filter::VisibleItem;
    use crate::model::{ArchiveKind, ArchiveStatus, PayloadMode, RowScope};
    use gpui_kit::Modifiers;
    use gpui_kit::component::IndexPath;

    #[test]
    fn tag_targets_follow_the_selection_like_delete_and_move() {
        let row_for_tag = |id: &str, name: &str| ArchiveRow {
            id: id.to_string(),
            name: name.to_string(),
            scope: crate::model::RowScope::Project,
            kind: ArchiveKind::File,
            version: 1,
            status: ArchiveStatus::Normal,
            tail: String::new(),
            alias: None,
            source_table: None,
            tags: Vec::new(),
            folder_id: None,
            updated_epoch: 0,
            archived_epoch: None,
            size_bytes: None,
        };
        // 单选：只给这一行（详情面板那一元入口也走同一口径）。
        let rows = [
            row_for_tag("ar_1", "月报"),
            row_for_tag("ar_2", "季报"),
            row_for_tag("ar_3", "年报"),
        ];
        let refs: Vec<&ArchiveRow> = rows.iter().collect();
        let single: std::collections::HashSet<String> = ["ar_2".to_string()].into_iter().collect();
        let one = tag_targets(&refs, &single, &rows[1]);
        assert_eq!(one.len(), 1);
        assert_eq!(one[0].name, "季报");

        // 多选：整选集（按可见行顺序，不是点中的那一条）。
        let multi: std::collections::HashSet<String> = ["ar_1".to_string(), "ar_3".to_string()]
            .into_iter()
            .collect();
        let targets = tag_targets(&refs, &multi, &rows[2]);
        let ids: Vec<&str> = targets.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids, vec!["ar_1", "ar_3"], "送整选集，且顺序按可见行");
        assert_eq!(targets[1].name, "年报", "显示名要跟着走（标题 / 回执要用）");

        // 多选集里但只选中了一条：那就是单选语义（与删除 / 移动同一条规矩）。
        assert_eq!(tag_targets(&refs, &multi, &rows[1]).len(), 1);
    }

    #[test]
    fn tag_entry_label_shows_the_scope() {
        assert_eq!(tag_entry_label(1), "编辑标签…");
        assert_eq!(tag_entry_label(3), "编辑标签（3 项）…");
    }

    #[test]
    fn row_click_classification_follows_the_prototype() {
        let none = Modifiers::default();
        // 次修饰键按平台取（macOS = Cmd，其余 = Ctrl）：测试不能写死一个平台。
        let secondary = if cfg!(target_os = "macos") {
            Modifiers {
                platform: true,
                ..none
            }
        } else {
            Modifiers {
                control: true,
                ..none
            }
        };

        assert_eq!(classify_click(1, none), RowClick::Select);
        assert_eq!(classify_click(2, none), RowClick::Open, "双击优先");
        assert_eq!(
            classify_click(
                1,
                Modifiers {
                    shift: true,
                    ..none
                }
            ),
            RowClick::Range
        );
        assert_eq!(classify_click(1, secondary), RowClick::Toggle);
        assert_eq!(
            classify_click(
                1,
                Modifiers {
                    shift: true,
                    ..secondary
                }
            ),
            RowClick::Range,
            "两个修饰键都按：区间优先"
        );
        assert_eq!(
            classify_click(
                2,
                Modifiers {
                    shift: true,
                    ..none
                }
            ),
            RowClick::Open,
            "双击带修饰键仍是打开"
        );
    }

    #[test]
    fn row_click_updates_the_selection_set() {
        let order: Vec<String> = ["ar_1", "ar_2", "ar_3"]
            .iter()
            .map(|id| id.to_string())
            .collect();
        let mut multi = Vec::new();
        let mut anchor = None;

        // 单击：单选（多选被重置）。
        assert_eq!(
            apply_row_click(&mut multi, &mut anchor, &order, "ar_2", RowClick::Select),
            Some("ar_2".to_string())
        );
        assert_eq!(multi, vec!["ar_2"]);

        // Ctrl 点击：加入，并按**可见行顺序**排（不是点击顺序）。
        apply_row_click(&mut multi, &mut anchor, &order, "ar_1", RowClick::Toggle);
        assert_eq!(multi, vec!["ar_1", "ar_2"]);
        assert_eq!(anchor.as_deref(), Some("ar_1"));

        // Ctrl 再点同一条：移出；最后一条被取消 = 无选中。
        apply_row_click(&mut multi, &mut anchor, &order, "ar_1", RowClick::Toggle);
        assert_eq!(multi, vec!["ar_2"]);
        let focus = apply_row_click(&mut multi, &mut anchor, &order, "ar_2", RowClick::Toggle);
        assert!(multi.is_empty() && focus.is_none());

        // Shift：从锚点到被点行的区间（反向也成立）。
        apply_row_click(&mut multi, &mut anchor, &order, "ar_1", RowClick::Select);
        apply_row_click(&mut multi, &mut anchor, &order, "ar_3", RowClick::Range);
        assert_eq!(multi, vec!["ar_1", "ar_2", "ar_3"]);
        apply_row_click(&mut multi, &mut anchor, &order, "ar_1", RowClick::Select);
        apply_row_click(&mut multi, &mut anchor, &order, "ar_2", RowClick::Select);
        apply_row_click(&mut multi, &mut anchor, &order, "ar_3", RowClick::Range);
        assert_eq!(multi, vec!["ar_2", "ar_3"], "锚点跟着最后一次单选走");

        // 锚点不在了（被筛掉）：区间退化为单选。
        let mut anchor = Some("ar_gone".to_string());
        let mut multi = vec!["ar_1".to_string()];
        apply_row_click(&mut multi, &mut anchor, &order, "ar_3", RowClick::Range);
        assert_eq!(multi, vec!["ar_3"]);

        // 双击：与单击同样落到单选（打开由行回调直接转给宿主）。
        let mut multi = vec!["ar_1".to_string(), "ar_2".to_string()];
        apply_row_click(&mut multi, &mut anchor, &order, "ar_3", RowClick::Open);
        assert_eq!(multi, vec!["ar_3"]);
    }

    #[test]
    fn enter_on_a_group_header_folds_that_group() {
        // 漫游位置落在分组头上 → Enter 是「折叠 / 展开」（头不是行，没有打开语义）；
        // 落在行上 / 越界 → 不是折叠（越界不能 panic：行集合刚变的那一帧会拿着旧索引）。
        let items = vec![
            VisibleItem::GroupHeader {
                key: crate::filter::GROUP_ALL.to_string(),
                label: "全部分组".to_string(),
                count: 1,
                depth: 0,
                collapsed: false,
            },
            VisibleItem::Row(ArchiveRow {
                id: "ar_1".to_string(),
                name: "a.sql".to_string(),
                alias: None,
                scope: crate::model::RowScope::Project,
                kind: ArchiveKind::File,
                version: 1,
                status: ArchiveStatus::Normal,
                tail: String::new(),
                source_table: None,
                tags: Vec::new(),
                folder_id: None,
                updated_epoch: 0,
                archived_epoch: None,
                size_bytes: None,
            }),
        ];
        assert_eq!(
            header_fold_key(&items, IndexPath::new(0)),
            Some(crate::filter::GROUP_ALL.to_string())
        );
        assert_eq!(
            header_fold_key(&items, IndexPath::new(1)),
            None,
            "行上是打开，不是折叠"
        );
        assert_eq!(
            header_fold_key(&items, IndexPath::new(9)),
            None,
            "越界不 panic"
        );
    }

    #[test]
    fn header_menu_actions_follow_the_prototype_order() {
        // 原型 §2.1 的 `⋯` 四项，自上而下：重建索引… / 打开资源目录 / 回收站… / 刷新。
        // 顺序与文案都是用户可见的约定，改动要有意识地改这条测试。
        let labels: Vec<&str> = HeaderMenuAction::ALL.iter().map(|a| a.label()).collect();
        assert_eq!(labels, vec!["重建索引…", "打开资源目录", "回收站…", "刷新"]);
        assert_eq!(HeaderMenuAction::ALL.len(), 4);
    }

    #[test]
    fn kind_icons_use_three_distinct_shapes() {
        // 形状区分（不靠颜色）：三个 kind 的图标互不相同，且都来自已捆绑的资产目录
        // （枚举由 gpui-kit-assets 的构建脚本按 svg 文件名生成，编译通过即资产存在）。
        let icons = [
            kind_icon(ArchiveKind::File),
            kind_icon(ArchiveKind::Analysis),
            kind_icon(ArchiveKind::TableRef),
        ];
        assert_eq!(icons[0], gpui_kit::assets::IconName::FileText);
        assert_eq!(icons[1], gpui_kit::assets::IconName::Table);
        assert_eq!(icons[2], gpui_kit::assets::IconName::ExternalLink);
        assert!(icons[0] != icons[1] && icons[1] != icons[2] && icons[0] != icons[2]);
    }

    #[test]
    fn strength_badge_follows_kind_and_status() {
        assert_eq!(
            strength_badge(ArchiveKind::File, ArchiveStatus::Normal),
            "已归档"
        );
        assert_eq!(
            strength_badge(ArchiveKind::Analysis, ArchiveStatus::Normal),
            "分析表"
        );
        assert_eq!(
            strength_badge(ArchiveKind::TableRef, ArchiveStatus::Normal),
            "引用"
        );
        // 异常态压过 kind。
        assert_eq!(
            strength_badge(ArchiveKind::File, ArchiveStatus::Missing),
            "缺失"
        );
        assert_eq!(
            strength_badge(ArchiveKind::File, ArchiveStatus::ContentChanged),
            "内容已变"
        );
    }

    #[test]
    fn badge_tone_marks_weak_strength_as_warning() {
        assert_eq!(
            badge_tone(ArchiveKind::File, ArchiveStatus::Normal),
            BadgeTone::Success
        );
        assert_eq!(
            badge_tone(ArchiveKind::Analysis, ArchiveStatus::Normal),
            BadgeTone::Info
        );
        // `引用`复现强度最弱：必须显眼（warning），不能与"已归档"同为成功色。
        assert_eq!(
            badge_tone(ArchiveKind::TableRef, ArchiveStatus::Normal),
            BadgeTone::Warning
        );
        assert_eq!(
            badge_tone(ArchiveKind::File, ArchiveStatus::Missing),
            BadgeTone::Danger
        );
    }

    /// 改名判据只有一处：`F2` 与右键「重命名…」共用它（多选不给改、只读项目不给改）。
    #[test]
    fn rename_needs_a_single_row_in_a_writable_project() {
        assert!(can_rename(0, false), "焦点行单独一条时可用");
        assert!(can_rename(1, false));
        assert!(!can_rename(2, false), "多选不给改（改哪一条都是猜）");
        assert!(!can_rename(1, true), "只读项目不给改");
    }

    /// 默认分组那一项的文案随状态翻转（同一个入口兼备“设”与“取消”）。
    #[test]
    fn default_group_entry_flips_between_set_and_clear() {
        assert_eq!(default_group_entry_label(false), "设为归档默认分组");
        assert_eq!(default_group_entry_label(true), "取消归档默认分组");
        assert_ne!(
            default_group_entry_label(false),
            default_group_entry_label(true)
        );
    }

    #[test]
    fn row_tail_prefers_detail_then_time_then_version() {
        assert_eq!(row_tail("1.2 KB", "3 天前", 3), "1.2 KB · 3 天前");
        assert_eq!(row_tail("1.2 KB", "", 3), "1.2 KB");
        assert_eq!(row_tail("", "3 天前", 3), "3 天前");
        assert_eq!(row_tail("", "", 3), "v3");
    }

    #[test]
    fn counts_line_hides_zero_buckets() {
        let clean = ArchiveCounts {
            total: 3,
            archived: 3,
            ..ArchiveCounts::default()
        };
        assert_eq!(clean.line(), "3 项 · 已归档 3");
        assert!(!clean.has_issues());
        // 加载中只加前缀，**不藏计数**（那是上一次的库口径，抹掉会让人以为库空了）。
        assert_eq!(clean.status_line(true), "加载中… · 3 项 · 已归档 3");
        assert_eq!(clean.status_line(false), clean.line());

        let with_issues = ArchiveCounts {
            total: 4,
            archived: 3,
            analysis: 1,
            missing: 1,
            drifted: 2,
            ..ArchiveCounts::default()
        };
        assert_eq!(
            with_issues.line(),
            "4 项 · 已归档 3 · 分析表 1 · 缺失 1 · 索引异常 2"
        );
        assert!(with_issues.has_issues());
    }

    /// 构造一条详情：只写与判定相关的字段，其余取最小可用值。
    fn detail(kind: ArchiveKind, status: ArchiveStatus, payload: Option<&str>) -> ArchiveDetail {
        ArchiveDetail {
            id: "ar_1".to_string(),
            name: "订单".to_string(),
            alias: None,
            kind,
            version: 1,
            status,
            readonly: true,
            payload_mode: PayloadMode::Copy,
            source_path: None,
            scope: RowScope::Project,
            size_label: String::new(),
            modified_label: String::new(),
            archived_label: String::new(),
            promoted_from: None,
            source_connection_id: None,
            source_table: None,
            content_hash: None,
            payload_rel_path: payload.map(str::to_string),
            history_label: String::new(),
            tags: Vec::new(),
            group: None,
            preview: crate::preview::Preview::default(),
        }
    }

    #[test]
    fn view_stats_only_for_data_sources_that_can_be_sampled() {
        let file =
            |payload: Option<&str>| detail(ArchiveKind::File, ArchiveStatus::Normal, payload);

        // 受管文件：本体扩展名得是 DuckDB 读得动的（大小写不敏感、中文名照认）。
        assert!(can_view_stats(&file(Some("orders.csv"))));
        assert!(can_view_stats(&file(Some("2026 预算.XLSX"))));
        assert!(can_view_stats(&file(Some("data/events.ndjson"))));
        // 非数据文件：`.sql` 是脚本不是数据，不给入口（不是“点了才说不行”）。
        assert!(!can_view_stats(&file(Some("dau.sql"))));
        assert!(!can_view_stats(&file(None)));
        // 缺失的行：本体不在，取不到数（即便扩展名可读）。
        assert!(!can_view_stats(&detail(
            ArchiveKind::File,
            ArchiveStatus::Missing,
            Some("orders.csv")
        )));

        // 远端引用：连接与表名都齐了才给（取样 SQL 由宿主按驱动拼）。
        let mut reference = detail(ArchiveKind::TableRef, ArchiveStatus::Normal, None);
        assert!(!can_view_stats(&reference), "没有表名就没有可取样的对象");
        reference.source_connection_id = Some("conn_1".to_string());
        assert!(!can_view_stats(&reference), "没有连接就不知道去哪取样");
        reference.source_table = Some("public.orders".to_string());
        assert!(can_view_stats(&reference));

        // 分析表：本体是 analytics.duckdb，要 ATTACH + 重建定义——后续批次接前不给入口。
        assert!(!can_view_stats(&detail(
            ArchiveKind::Analysis,
            ArchiveStatus::Normal,
            Some("analytics.duckdb")
        )));
    }

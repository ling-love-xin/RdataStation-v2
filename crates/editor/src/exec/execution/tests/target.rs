//! 执行测试 —— 执行目标解析（selection/statement/batch/segment/pushdown）
//!
//! 自 execution.rs tests 拆分（2026-10-29 Step 6）。
//!
use super::*;

    // ===== 执行目标解析 =====

    #[test]
    fn selection_wins_over_the_statement_under_the_cursor() {
        let text = "select 1;\nselect 2;";
        // 光标在第一句里，但有选区（第二句的一部分）
        let target = resolve_target(text, 10..18);
        assert_eq!(target, ExecTarget::Selection("select 2".to_string()));
        assert_eq!(target.label(), "选区");
    }

    #[test]
    fn blank_selection_falls_back_to_the_statement_under_the_cursor() {
        let text = "select 1;\nselect 2;";
        // 光标在第二句里，选区是空白
        let target = resolve_target(text, 12..12);
        assert_eq!(target, ExecTarget::Statement("select 2".to_string()));
        assert_eq!(target.label(), "当前语句");
    }

    /// 菜单里选“执行当前语句”是明确意图：**有选区也不改目标**（那是“执行选区”的语义）
    #[test]
    fn statement_target_ignores_an_active_selection() {
        let text = "select 1;\nselect 2;";
        // 光标在第一句里，但同时有选区（第二句）——两条路径必须给出不同目标
        assert_eq!(
            resolve_target(text, 10..18),
            ExecTarget::Selection("select 2".to_string())
        );
        assert_eq!(
            statement_target(text, 3),
            ExecTarget::Statement("select 1".to_string())
        );
        // 越界光标不 panic，收敛到末句（与 resolve_target 同一口径）
        assert_eq!(
            statement_target(text, 9999),
            ExecTarget::Statement("select 2".to_string())
        );
        assert_eq!(statement_target("-- 只有注释\n", 3), ExecTarget::Empty);
    }

    #[test]
    fn cursor_inside_the_first_statement_picks_it() {
        let text = "select 1;\nselect 2;";
        let target = resolve_target(text, 3..3);
        assert_eq!(target, ExecTarget::Statement("select 1".to_string()));
    }

    #[test]
    fn cursor_in_the_gap_picks_the_next_statement() {
        // 光标停在第 1、2 句之间的空行上 → 跑下面那条（主流客户端行为）
        let text = "select 1;\n\nselect 2;";
        let target = resolve_target(text, 10..10);
        assert_eq!(target, ExecTarget::Statement("select 2".to_string()));
    }

    #[test]
    fn cursor_after_the_last_statement_keeps_it() {
        let text = "select 1;\nselect 2;";
        let target = resolve_target(text, text.len()..text.len());
        assert_eq!(target, ExecTarget::Statement("select 2".to_string()));
    }

    #[test]
    fn semicolons_inside_strings_do_not_split_the_statement() {
        let text = "select ';' as a;\nselect 2";
        let target = resolve_target(text, 5..5);
        assert_eq!(target, ExecTarget::Statement("select ';' as a".to_string()));
    }

    /// 菜单是**显式目标**：各项各自独立，不互相推断；“执行选区”没选区就置灰
    #[test]
    fn menu_items_resolve_their_own_targets() {
        let text = "select 1;\nselect 2;";
        // 只选中第二句的一部分：菜单里的“当前语句”应当给**整句**，
        // “执行选区”才给那段片段（后者会报语法错，但那正是用户当下选的东西）
        let partial = 12..18;
        let statements = batch_target(text).statements().len();

        assert_eq!(
            target_for_menu(ExecMenuKind::CurrentStatement, text, partial.clone()),
            ExecTarget::Statement("select 2".to_string()),
            "菜单选“当前语句”时不得去执行半截选区"
        );
        assert_eq!(
            target_for_menu(ExecMenuKind::Selection, text, partial.clone()),
            ExecTarget::Selection("lect 2".to_string())
        );
        assert_eq!(
            target_for_menu(ExecMenuKind::All, text, partial.clone()),
            ExecTarget::All(text.to_string())
        );

        assert!(ExecMenuKind::Selection.is_available(&partial, statements));
        assert!(!ExecMenuKind::Selection.is_available(&(4..4), statements));
        assert!(ExecMenuKind::CurrentStatement.is_available(&(4..4), statements));
        assert!(ExecMenuKind::All.is_available(&(4..4), statements));

        // 五项文案彼此不同（菜单上看不出区别就是没实现），顺序也固定
        let labels: Vec<&str> = ExecMenuKind::ALL.iter().map(|kind| kind.label()).collect();
        assert_eq!(labels.len(), 5);
        assert!(labels.iter().all(|label| !label.is_empty()));
        let unique: std::collections::BTreeSet<_> = labels.iter().collect();
        assert_eq!(unique.len(), labels.len(), "菜单项文案不得重复：{labels:?}");

        // 空文档 / 只有注释：各项都得是 Empty，不得拿注释去 prepare
        let comments = "-- 只是注释\n";
        for kind in ExecMenuKind::ALL {
            assert_eq!(
                target_for_menu(kind, comments, 3..3),
                ExecTarget::Empty,
                "{kind:?} 对纯注释文档应无可执行内容"
            );
        }
    }

    /// 批量：按语句切开、**逐条独立**（空白与纯注释段不是可执行内容，不进列表）
    #[test]
    fn batch_target_splits_the_script_into_independent_statements() {
        let text = "select 1;\n\nselect 2;\nselect 3";
        let target = batch_target(text);
        assert_eq!(
            target.statements(),
            vec![
                "select 1".to_string(),
                "select 2".to_string(),
                "select 3".to_string()
            ]
        );
        assert_eq!(target.label(), "批量");
        assert!(target.sql().is_none(), "批量不走单条 SQL 那条路");

        // 语句**前面**的注释跟着它一起走（与“执行当前语句”拿到的是同一份文本）
        let commented = batch_target("select 1;\n-- 说明\nselect 2;");
        assert_eq!(commented.statements().len(), 2);
        assert!(
            commented.statements()[1].ends_with("select 2"),
            "注释属于它后面那句：{:?}",
            commented.statements()
        );

        // 字符串里的分号不算切分点（与“执行当前语句”同一套词法级判据）
        assert_eq!(
            batch_target("select ';' as a; select 2").statements().len(),
            2
        );

        // 不足两条语句 / 纯注释：拿不到“批”
        assert_eq!(batch_target("select 1").statements().len(), 1);
        assert!(
            !ExecMenuKind::Batch.is_available(&(4..4), 1),
            "只有一句时“批量执行”该置灰"
        );
        assert!(ExecMenuKind::Batch.is_available(&(4..4), 2));
        assert_eq!(batch_target("-- 只是注释"), ExecTarget::Empty);
    }

    /// 落位：全部执行项都往新结果集放（每次执行 = 一个新结果 tab；2026-10-27 起默认执行
    /// 不再覆盖当前 tab——用户反馈「一个 tab 容纳了好多结果」）
    #[test]
    fn batch_and_new_set_land_in_new_result_sets() {
        assert_eq!(ExecMenuKind::Batch.placement(), ResultPlacement::NewSet);
        assert_eq!(ExecMenuKind::NewSet.placement(), ResultPlacement::NewSet);
        for kind in [
            ExecMenuKind::CurrentStatement,
            ExecMenuKind::Selection,
            ExecMenuKind::All,
        ] {
            assert_eq!(kind.placement(), ResultPlacement::NewSet, "{kind:?}");
        }

        // “在新结果标签中执行”只换落位：目标就是主按钮那一套（选区优先）
        let text = "select 1;\nselect 2;";
        assert_eq!(
            target_for_menu(ExecMenuKind::NewSet, text, 10..18),
            resolve_target(text, 10..18)
        );
        assert_eq!(
            target_for_menu(ExecMenuKind::NewSet, text, 3..3),
            ExecTarget::Statement("select 1".to_string())
        );
        assert!(
            ExecMenuKind::NewSet.is_available(&(4..4), 0),
            "没选区也能新开一份结果"
        );
    }

    #[test]
    fn comments_only_document_has_nothing_to_run() {
        let text = "-- 只是注释\n\n";
        assert_eq!(resolve_target(text, 5..5), ExecTarget::Empty);
        assert!(resolve_target(text, 5..5).sql().is_none());
        assert_eq!(all_target(text), ExecTarget::Empty);
    }

    #[test]
    fn empty_document_has_nothing_to_run() {
        assert_eq!(resolve_target("", 0..0), ExecTarget::Empty);
    }

    #[test]
    fn all_target_keeps_the_whole_script() {
        let text = "select 1;\nselect 2;";
        assert_eq!(all_target(text), ExecTarget::All(text.to_string()));
        assert_eq!(all_target(text).sql(), Some(text));
        assert_eq!(all_target(text).label(), "全部");
    }

    #[test]
    fn selection_out_of_range_does_not_panic() {
        let text = "select 1";
        // 越界 + 落在多字节字符中间的选区都要能收敛
        let target = resolve_target("select '中文'", 7..99);
        assert_eq!(target, ExecTarget::Selection("'中文'".to_string()));
        assert_eq!(
            resolve_target(text, 100..200),
            ExecTarget::Statement(text.to_string())
        );
    }

    /// 【B5b】取下一段是个**显式目标**：带 (offset, limit)，标签可读，SQL 就是原 SQL
    #[test]
    fn a_segment_target_carries_its_offset_and_limit() {
        let target = ExecTarget::Segment {
            sql: "select n from t".to_string(),
            offset: 1_000,
            limit: SEGMENT_ROWS,
        };
        assert_eq!(target.segment(), Some((1_000, SEGMENT_ROWS)));
        assert_eq!(target.statements(), ["select n from t".to_string()]);
        assert_eq!(target.sql(), Some("select n from t"));
        assert_eq!(target.label(), "取下一段");
        assert_eq!(target.lineage(), "取下一段");

        // 其它目标没有分段语义（不会误走 fetch_next）
        assert_eq!(all_target("select 1").segment(), None);
    }

    /// 【A2】下发载荷还原成形成原料：filtered / sorted_down 互斥，只有其一
    #[test]
    fn pushdown_round_trips_from_the_job_payload() {
        use crate::store::Pushdown;
        assert_eq!(
            Pushdown::from_job(
                &Some(("ab".to_string(), vec!["id".to_string(), "name".to_string()])),
                &None,
            ),
            Some(Pushdown::Filtered {
                filter: "ab".to_string(),
                columns: vec!["id".to_string(), "name".to_string()],
            })
        );
        assert_eq!(
            Pushdown::from_job(&None, &Some(("created_at".to_string(), true))),
            Some(Pushdown::SortedDown {
                column: "created_at".to_string(),
                descending: true,
            })
        );
        // 两个都没有 = 普通执行（没有可保持的形态）
        assert_eq!(Pushdown::from_job(&None, &None), None);
        // 同时有（不该发生）→ 按 filtered 优先（一次只有一个下发载荷）
        let both = Pushdown::from_job(
            &Some(("ab".to_string(), vec!["id".to_string()])),
            &Some(("id".to_string(), false)),
        );
        assert!(matches!(both, Some(Pushdown::Filtered { .. })));
    }

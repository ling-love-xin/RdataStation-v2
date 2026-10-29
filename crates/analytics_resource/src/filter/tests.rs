    use super::{ResourcesFilter, SortField, SortOrder, apply_view, needs_attention};
    use crate::detail_view::ArchiveTagChip;
    use crate::model::{ArchiveKind, ArchiveStatus};
    use crate::resource_view::ArchiveRow;

    fn row(
        id: &str,
        name: &str,
        kind: ArchiveKind,
        status: ArchiveStatus,
        version: i32,
        tail: &str,
    ) -> ArchiveRow {
        ArchiveRow {
            id: id.to_string(),
            name: name.to_string(),
            alias: None,
            scope: crate::model::RowScope::Project,
            kind,
            version,
            status,
            tail: tail.to_string(),
            source_table: None,
            tags: Vec::new(),
            folder_id: None,
            // 原始值默认给中性值：只有排序用例关心它们（需要时用结构体更新语法盖掉）。
            updated_epoch: 0,
            archived_epoch: None,
            size_bytes: None,
        }
    }

    fn row_with_tags(id: &str, tags: &[&str]) -> ArchiveRow {
        ArchiveRow {
            // 测试里标签名 = id（名字与 id 的对应关系不是本模块的事）。
            tags: tags
                .iter()
                .map(|t| ArchiveTagChip {
                    id: t.to_string(),
                    name: t.to_string(),
                })
                .collect(),
            ..row(id, id, ArchiveKind::File, ArchiveStatus::Normal, 1, "")
        }
    }

    fn sample() -> Vec<ArchiveRow> {
        vec![
            row(
                "ar_2",
                "Beta.sql",
                ArchiveKind::File,
                ArchiveStatus::Normal,
                2,
                "1.2 KB · 3 天前",
            ),
            row(
                "ar_1",
                "alpha.sql",
                ArchiveKind::Analysis,
                ArchiveStatus::Normal,
                5,
                "12,480 行 × 18 列",
            ),
            row(
                "ar_3",
                "gamma.sql",
                ArchiveKind::File,
                ArchiveStatus::Missing,
                1,
                "",
            ),
        ]
    }

    #[test]
    fn query_matches_name_and_tail_case_insensitively() {
        let rows = sample();

        let by_name = ResourcesFilter {
            query: "ALPHA".to_string(),
            ..ResourcesFilter::default()
        };
        let hit = apply_view(&rows, &by_name, SortField::Name, SortOrder::Asc);
        assert_eq!(hit.len(), 1);
        assert_eq!(hit[0].id, "ar_1");

        // 用户也会拿尾部线索找（"1.2"、"48" 之类）。
        let by_tail = ResourcesFilter {
            query: "1.2".to_string(),
            ..ResourcesFilter::default()
        };
        assert_eq!(
            apply_view(&rows, &by_tail, SortField::Name, SortOrder::Asc).len(),
            1
        );
    }

    /// 匹配面比展示面宽（原型 §2.2）：别名 / 标签名 / 来源表都可搜。
    #[test]
    fn query_matches_alias_tag_name_and_source_table() {
        let mut alias_row = row(
            "ar_1",
            "dau_report.sql",
            ArchiveKind::Analysis,
            ArchiveStatus::Normal,
            1,
            "",
        );
        alias_row.alias = Some("月报".to_string());

        let mut tag_row = row_with_tags("ar_2", &["at_1"]);
        tag_row.tags[0].name = "财务报表".to_string();

        let mut table_row = row(
            "ar_3",
            "orders_archive.sql",
            ArchiveKind::TableRef,
            ArchiveStatus::Normal,
            1,
            "无指纹",
        );
        table_row.source_table = Some("dwd.dwd_orders".to_string());

        let rows = vec![alias_row, tag_row, table_row];
        let hit = |query: &str| {
            let filter = ResourcesFilter {
                query: query.to_string(),
                ..ResourcesFilter::default()
            };
            apply_view(&rows, &filter, SortField::Name, SortOrder::Asc)
                .iter()
                .map(|row| row.id.clone())
                .collect::<Vec<_>>()
        };

        assert_eq!(hit("月报"), vec!["ar_1"], "别名命中（行上不显示它）");
        assert_eq!(
            hit("财务报表"),
            vec!["ar_2"],
            "标签名命中（筛标签维比的是 id）"
        );
        assert_eq!(hit("DWD_ORDERS"), vec!["ar_3"], "来源表命中且大小写不敏感");
        assert_eq!(hit("dwd.dwd"), vec!["ar_3"], "来源表可整串搜");

        // 跨字段的偶然连缀不算命中（字段之间留了空格），无关词一律不命中。
        assert!(hit("sql 无").is_empty());
        assert!(hit("zzz").is_empty());
    }

    /// 匹配面是**加**进去的而不是替换：缺别名 / 标签 / 来源表的行依旧按名称与尾巴命中。
    #[test]
    fn search_haystack_keeps_name_and_tail_for_every_row() {
        let row = row(
            "ar_1",
            "alpha.sql",
            ArchiveKind::File,
            ArchiveStatus::Normal,
            3,
            "1.2 KB · 3 天前",
        );
        let haystack = super::search_haystack(&row);
        assert!(haystack.contains("alpha.sql"));
        assert!(haystack.contains("1.2 kb"), "尾巴已小写化：{haystack}");
        assert_eq!(haystack, haystack.to_lowercase(), "装配即小写");
    }

    /// 给用户看的那份字段清单（[`super::SEARCH_FIELDS`]）与实现真的对得上：
    /// 每一项都能在某行上搜到——“说明了却搜不到”比不说还糟。
    #[test]
    fn search_fields_list_covers_what_the_haystack_really_matches() {
        let mut row = row(
            "ar_1",
            "dau_report.sql",
            ArchiveKind::TableRef,
            ArchiveStatus::Normal,
            3,
            "1.2 KB · 3 天前",
        );
        row.alias = Some("月报".to_string());
        row.source_table = Some("dwd.dwd_orders".to_string());
        row.tags = vec![ArchiveTagChip {
            id: "at_1".to_string(),
            name: "财务报表".to_string(),
        }];
        let haystack = super::search_haystack(&row);

        // 与 `SEARCH_FIELDS` 一一对应（顺序即清单顺序）。
        let probes = ["dau_report", "月报", "财务报表", "dwd.dwd_orders", "1.2 kb"];
        assert_eq!(super::SEARCH_FIELDS.len(), probes.len());
        for (field, probe) in super::SEARCH_FIELDS.iter().zip(probes) {
            assert!(haystack.contains(probe), "{field} 应可被搜到（{probe}）");
        }
    }

    /// 空态副文案：搜不到时告知能搜哪些字段，纯筛选无匹配时改指筛选。
    #[test]
    fn no_match_hint_speaks_about_the_query_only_when_there_is_one() {
        let filtered = super::no_match_hint("  ");
        assert!(
            filtered.contains("筛选"),
            "没搜索词就不谈搜什么：{filtered}"
        );
        assert!(!filtered.contains("别名"));

        let searched = super::no_match_hint("月报");
        for field in super::SEARCH_FIELDS {
            assert!(searched.contains(field), "缺字段说明：{field} / {searched}");
        }
    }

    #[test]
    fn kind_and_issue_filters_narrow_the_list() {
        let rows = sample();

        let only_files = ResourcesFilter {
            kinds: vec![ArchiveKind::File],
            ..ResourcesFilter::default()
        };
        assert_eq!(
            apply_view(&rows, &only_files, SortField::Name, SortOrder::Asc).len(),
            2
        );
        assert!(!only_files.is_empty());

        let issues = ResourcesFilter {
            only_issues: true,
            ..ResourcesFilter::default()
        };
        let hit = apply_view(&rows, &issues, SortField::Name, SortOrder::Asc);
        assert_eq!(hit.len(), 1);
        assert!(needs_attention(&hit[0]));
        assert_eq!(hit[0].id, "ar_3");

        // 全空条件是"空"，面板据此区分"无匹配"与"还没有任何存档"。
        assert!(ResourcesFilter::default().is_empty());
    }

    #[test]
    fn sort_is_stable_and_direction_aware() {
        let rows = sample();
        let none = ResourcesFilter::default();

        let asc = apply_view(&rows, &none, SortField::Name, SortOrder::Asc);
        assert_eq!(
            asc.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
            vec!["ar_1", "ar_2", "ar_3"],
            "名称升序且大小写不敏感"
        );

        let desc = apply_view(&rows, &none, SortField::Name, SortOrder::Desc);
        assert_eq!(
            desc.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
            vec!["ar_3", "ar_2", "ar_1"]
        );

        let by_version = apply_view(&rows, &none, SortField::Version, SortOrder::Desc);
        assert_eq!(
            by_version.iter().map(|r| r.version).collect::<Vec<_>>(),
            vec![5, 2, 1]
        );
    }

    /// 造一条带原始值的行（名称故意与时间 / 体积反着排，这样“按名称兜底”不会被误认为排对了）。
    fn row_with_raw(
        id: &str,
        name: &str,
        updated: i64,
        archived: Option<i64>,
        size: Option<i64>,
    ) -> ArchiveRow {
        ArchiveRow {
            updated_epoch: updated,
            archived_epoch: archived,
            size_bytes: size,
            ..row(id, name, ArchiveKind::File, ArchiveStatus::Normal, 1, "")
        }
    }

    #[test]
    fn time_and_size_sorts_use_the_raw_values() {
        let rows = vec![
            row_with_raw("ar_1", "a_big.sql", 300, Some(30), Some(1_228_800)),
            row_with_raw("ar_2", "z_small.sql", 100, Some(10), Some(900)),
            row_with_raw("ar_3", "m_mid.sql", 200, Some(20), Some(4096)),
        ];
        let none = ResourcesFilter::default();
        let ids = |sorted: Vec<ArchiveRow>| sorted.iter().map(|r| r.id.clone()).collect::<Vec<_>>();

        // 时间：新的在前（降序是这一列的常态口径）。
        assert_eq!(
            ids(apply_view(
                &rows,
                &none,
                SortField::UpdatedAt,
                SortOrder::Desc
            )),
            vec!["ar_1", "ar_3", "ar_2"]
        );
        assert_eq!(
            ids(apply_view(
                &rows,
                &none,
                SortField::ArchivedAt,
                SortOrder::Asc
            )),
            vec!["ar_2", "ar_3", "ar_1"]
        );

        // 大小按字节比：900 B 在 1.2 KB 之前（若拿尾巴字符串比就会反过来）。
        assert_eq!(
            ids(apply_view(&rows, &none, SortField::Size, SortOrder::Asc)),
            vec!["ar_2", "ar_3", "ar_1"]
        );
    }

    #[test]
    fn unknown_time_and_size_rows_stay_last_in_both_directions() {
        let rows = vec![
            row_with_raw("ar_unknown", "a_unknown.sql", 0, None, None),
            row_with_raw("ar_small", "z_small.sql", 100, Some(10), Some(900)),
            row_with_raw("ar_big", "m_big.sql", 200, Some(20), Some(4096)),
        ];
        let none = ResourcesFilter::default();
        let ids = |sorted: Vec<ArchiveRow>| sorted.iter().map(|r| r.id.clone()).collect::<Vec<_>>();

        // 缺值不随方向翻转：升序降序都在末尾（“不知道”不当“最大”）。
        assert_eq!(
            ids(apply_view(&rows, &none, SortField::Size, SortOrder::Asc)),
            vec!["ar_small", "ar_big", "ar_unknown"]
        );
        assert_eq!(
            ids(apply_view(&rows, &none, SortField::Size, SortOrder::Desc)),
            vec!["ar_big", "ar_small", "ar_unknown"]
        );
        assert_eq!(
            ids(apply_view(
                &rows,
                &none,
                SortField::ArchivedAt,
                SortOrder::Desc
            )),
            vec!["ar_big", "ar_small", "ar_unknown"]
        );
    }

    /// 同一排序值（含两个都缺值）时按名称兜底，且兜底键不随方向翻转。
    #[test]
    fn ties_fall_back_to_name_without_flipping() {
        let rows = vec![
            row_with_raw("ar_b", "beta.sql", 0, None, None),
            row_with_raw("ar_a", "Alpha.sql", 0, None, None),
        ];
        let none = ResourcesFilter::default();

        for order in [SortOrder::Asc, SortOrder::Desc] {
            let sorted = apply_view(&rows, &none, SortField::ArchivedAt, order);
            assert_eq!(
                sorted.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
                vec!["ar_a", "ar_b"],
                "兜底键恒为名称升序（{order:?}）"
            );
        }
    }

    #[test]
    fn order_flips_on_repeat_click() {
        assert_eq!(SortOrder::Asc.flipped(), SortOrder::Desc);
        assert_eq!(SortOrder::Desc.flipped(), SortOrder::Asc);
        assert_eq!(SortField::Name.label(), "名称");
        // 菜单按原型 §2.2 的顺序：名称 / 归档时间 / 更新时间 / 大小 / 版本号。
        assert_eq!(
            SortField::ALL.map(SortField::label),
            ["名称", "归档时间", "更新时间", "大小", "版本号"]
        );
        // 工具栏按钮用两字短文案（240px 面板里要跟搜索框分宽度）。
        assert_eq!(
            SortField::ALL.map(SortField::short_label),
            ["名称", "归档", "更新", "大小", "版本"]
        );
        // 落盘 key 与菜单文案分家：文案改文案、key 不改（设置项 `resources.default_sort` 存它）。
        assert_eq!(
            SortField::ALL.map(SortField::key),
            ["name", "archived_at", "updated_at", "size", "version"]
        );
        for field in SortField::ALL {
            assert_eq!(SortField::from_key(field.key()), Some(field));
        }
        assert_eq!(SortField::from_key("看不见的列"), None, "未知 key 不猜");
        assert_eq!(SortField::Name.default_order(), SortOrder::Asc);
        assert_eq!(SortField::UpdatedAt.default_order(), SortOrder::Desc);
        assert_eq!(SortOrder::Asc.arrow(), "↑");
        assert_eq!(SortOrder::Desc.arrow(), "↓");
    }

    #[test]
    fn toggle_kind_normalizes_select_all_to_unlimited() {
        let mut filter = ResourcesFilter::default();

        filter.toggle_kind(ArchiveKind::File);
        filter.toggle_kind(ArchiveKind::Analysis);
        assert!(filter.has_kind(ArchiveKind::File));
        assert_eq!(filter.menu_dims(), 2);
        assert!(!filter.is_empty(), "选了两种就是真筛选");

        // 三个全选 = 不限：不能留下"看起来在筛选"的等价条件。
        filter.toggle_kind(ArchiveKind::TableRef);
        assert!(filter.kinds.is_empty());
        assert!(filter.is_empty());
        assert!(!filter.has_kind(ArchiveKind::TableRef));

        // 再点一次就取消勾选（不是又选中）。
        filter.toggle_kind(ArchiveKind::File);
        filter.toggle_kind(ArchiveKind::File);
        assert!(!filter.has_kind(ArchiveKind::File));
    }

    #[test]
    fn menu_dims_counts_menu_conditions_only() {
        let mut filter = ResourcesFilter {
            query: "dau".to_string(),
            ..ResourcesFilter::default()
        };
        // 搜索词不算徽标（它在输入框里看得见）。
        assert_eq!(filter.menu_dims(), 0);
        filter.only_issues = true;
        assert_eq!(filter.menu_dims(), 1);
    }

    /// 标签筛选：多选是 OR（“这两个标签的存档都看”），没有“全选 = 不限”的归一化。
    #[test]
    fn tag_filter_is_multi_select_or() {
        let rows = vec![
            row_with_tags("ar_1", &["at_a"]),
            row_with_tags("ar_2", &["at_b"]),
            row_with_tags("ar_3", &["at_a", "at_b"]),
            row_with_tags("ar_4", &[]),
        ];
        let mut filter = ResourcesFilter::default();
        assert_eq!(
            apply_view(&rows, &filter, SortField::Name, SortOrder::Asc).len(),
            4
        );

        filter.toggle_tag("at_a");
        assert!(filter.has_tag("at_a"));
        assert!(
            !filter.is_empty(),
            "选了标签就是真筛选（空库不该显示“没有匹配”）"
        );
        assert_eq!(
            apply_view(&rows, &filter, SortField::Name, SortOrder::Asc)
                .iter()
                .map(|r| r.id.as_str())
                .collect::<Vec<_>>(),
            vec!["ar_1", "ar_3"]
        );

        // 再选一个 = 并集（不是交集）。
        filter.toggle_tag("at_b");
        assert_eq!(
            apply_view(&rows, &filter, SortField::Name, SortOrder::Asc).len(),
            3
        );
        assert_eq!(filter.menu_dims(), 2);

        // 再点一次取消勾选。
        filter.toggle_tag("at_a");
        assert!(!filter.has_tag("at_a"));
        assert_eq!(
            apply_view(&rows, &filter, SortField::Name, SortOrder::Asc).len(),
            2
        );
    }

    /// 标签被删后要能抹掉悬空条件（否则列表会“什么都没匹配”，而菜单上的勾还在）。
    #[test]
    fn drop_tag_removes_a_stale_condition() {
        let mut filter = ResourcesFilter::default();
        filter.toggle_tag("at_a");
        filter.drop_tag("at_a");
        assert!(filter.is_empty());
        assert_eq!(filter.menu_dims(), 0);
    }

    // ==================== 分组折叠区 ====================

    use super::{GROUP_ALL, GROUP_UNGROUPED, VisibleItem, build_visible_items};
    use crate::resource_view::GroupOption;

    fn row_in(id: &str, folder: Option<&str>) -> ArchiveRow {
        ArchiveRow {
            folder_id: folder.map(str::to_string),
            ..row(id, id, ArchiveKind::File, ArchiveStatus::Normal, 1, "")
        }
    }

    fn group(id: &str, name: &str, count: usize) -> GroupOption {
        let _ = count;
        GroupOption {
            id: id.to_string(),
            name: name.to_string(),
        }
    }

    fn keys(items: &[VisibleItem]) -> Vec<String> {
        items
            .iter()
            .map(|item| match item {
                VisibleItem::GroupHeader { key, .. } => key.clone(),
                VisibleItem::Row(row) => row.id.clone(),
            })
            .collect()
    }

    /// 没有分组就不出头：空库与“全部未分组”不该多两行噪声。
    #[test]
    fn no_groups_means_no_headers() {
        let rows = vec![row_in("ar_1", None), row_in("ar_2", None)];
        let items = build_visible_items(&rows, &[], &Default::default());
        assert_eq!(keys(&items), vec!["ar_1", "ar_2"]);
    }

    /// 三层结构：全部分组 → 未分组 → 各分组；计数来自行集合，不另查库。
    #[test]
    fn groups_partition_rows_with_counts() {
        let rows = vec![
            row_in("ar_1", Some("af_1")),
            row_in("ar_2", None),
            row_in("ar_3", Some("af_1")),
            row_in("ar_4", Some("af_2")),
        ];
        let groups = vec![group("af_1", "月报", 0), group("af_2", "周报", 0)];
        let items = build_visible_items(&rows, &groups, &Default::default());
        assert_eq!(
            keys(&items),
            vec![
                GROUP_ALL,
                GROUP_UNGROUPED,
                "ar_2",
                "af_1",
                "ar_1",
                "ar_3",
                "af_2",
                "ar_4",
            ]
        );
        let total = items
            .iter()
            .find_map(|item| match item {
                VisibleItem::GroupHeader { key, count, .. } if key == GROUP_ALL => Some(*count),
                _ => None,
            })
            .expect("全部分组头");
        assert_eq!(total, 4, "“全部分组”数的是可见行总数");
    }

    /// 折叠：头还在（否则展不开），行不出场；折“全部分组”就只剩那一行。
    #[test]
    fn collapsing_hides_rows_but_keeps_headers() {
        let rows = vec![row_in("ar_1", Some("af_1")), row_in("ar_2", None)];
        let groups = vec![group("af_1", "月报", 1)];

        let collapsed: std::collections::HashSet<String> =
            ["af_1".to_string()].into_iter().collect();
        let items = build_visible_items(&rows, &groups, &collapsed);
        assert_eq!(
            keys(&items),
            vec![GROUP_ALL, GROUP_UNGROUPED, "ar_2", "af_1"],
            "折叠的分组只出头不出行"
        );

        let all_collapsed: std::collections::HashSet<String> =
            [GROUP_ALL.to_string()].into_iter().collect();
        let items = build_visible_items(&rows, &groups, &all_collapsed);
        assert_eq!(keys(&items), vec![GROUP_ALL], "折全部 = 只剩一行");
    }

    /// 计数按**当前可见行**现算：筛选后头里的数就是眼前的行数。
    #[test]
    fn header_counts_follow_the_visible_rows() {
        let groups = vec![group("af_1", "月报", 99)];
        let rows = vec![row_in("ar_1", Some("af_1")), row_in("ar_2", Some("af_1"))];
        let items = build_visible_items(&rows, &groups, &Default::default());
        let counts: Vec<usize> = items
            .iter()
            .filter_map(|item| match item {
                VisibleItem::GroupHeader { count, .. } => Some(*count),
                _ => None,
            })
            .collect();
        assert_eq!(counts, vec![2, 0, 2], "全部分组 / 未分组 / 月报");
    }

    /// 脏数据（分组被删而关联还在）：归属认不出就算未分组，不能把行吞掉。
    #[test]
    fn unknown_folders_fall_back_to_ungrouped() {
        let groups = vec![group("af_1", "月报", 0)];
        let rows = vec![
            row_in("ar_1", Some("af_gone")),
            row_in("ar_2", Some("af_1")),
        ];
        let items = build_visible_items(&rows, &groups, &Default::default());
        assert_eq!(
            keys(&items),
            vec![GROUP_ALL, GROUP_UNGROUPED, "ar_1", "af_1", "ar_2"],
            "认不出的分组归属算未分组（行不丢）"
        );
    }

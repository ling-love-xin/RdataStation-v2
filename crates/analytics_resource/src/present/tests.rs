    use super::{
        ArchiveStatuses, FolderMemberships, PreviewMap, ResourceTags, SnapshotInputs,
        VersionCounts, build_repair_rows, build_snapshot, build_trash_snapshot, build_version_rows,
        format_relative_time, format_scale, format_size, format_timestamp, tail_for,
    };
    use crate::dialogs::index_repair::RepairGroup;
    use crate::model::{ArchiveKind, ArchiveStatus, ORIGIN_RESOURCES};
    use crate::models::{AnalyticsResource, ResourceVersion};
    use crate::resource_view::{GroupOption, TagOption};
    use crate::{IndexIssue, IndexScanReport};
    use chrono::{DateTime, Duration, Utc};
    use engine::persistence::trash::{TrashEntry, TrashKind};
    use serde_json::Value;

    /// 空的标签 / 分组映射：测试里反复要，做成静态的（借用的生周期才够长）。
    fn empty_tags() -> &'static ResourceTags {
        static EMPTY: std::sync::OnceLock<ResourceTags> = std::sync::OnceLock::new();
        EMPTY.get_or_init(ResourceTags::new)
    }

    fn empty_folders() -> &'static FolderMemberships {
        static EMPTY: std::sync::OnceLock<FolderMemberships> = std::sync::OnceLock::new();
        EMPTY.get_or_init(FolderMemberships::new)
    }

    /// 空预览映射：大多数用例不关心预览（要断言预览的用例自己造一份）。
    fn empty_previews() -> &'static PreviewMap {
        static EMPTY: std::sync::OnceLock<PreviewMap> = std::sync::OnceLock::new();
        EMPTY.get_or_init(PreviewMap::new)
    }

    fn row_model(id: &str, kind: &str, file_size: Option<i32>) -> AnalyticsResource {
        let now = Utc::now();
        AnalyticsResource {
            id: id.to_string(),
            resource_type: kind.to_string(),
            name: format!("{id}.sql"),
            alias: None,
            config: Value::Null,
            scope: "project".to_string(),
            row_count: None,
            column_count: None,
            file_size,
            version: 1,
            parent_version_id: None,
            parent_resource_id: None,
            source_query: None,
            created_at: now,
            updated_at: now,
            created_by: None,
            deleted_at: None,
            kind: kind.to_string(),
            content_hash: Some("abc".to_string()),
            file_rel_path: Some(format!("{id}.sql")),
            payload_mode: crate::model::PayloadMode::Copy.as_db_str().to_string(),
            source_path: None,
            readonly: 1,
            promoted_from: None,
            source_connection_id: None,
            source_table: None,
            definition_sql: None,
            archived_at: Some(now),
        }
    }

    fn trash_entry(
        id: &str,
        name: &str,
        origin: &str,
        rel_path: &str,
        kind: TrashKind,
        size: u64,
    ) -> TrashEntry {
        TrashEntry {
            manifest: engine::persistence::trash::TrashManifest {
                id: id.to_string(),
                name: name.to_string(),
                origin: origin.to_string(),
                original_rel_path: rel_path.to_string(),
                kind,
                deleted_at: Utc::now(),
                size,
            },
        }
    }

    /// 回收站快照只收本模块的条目；别人的只给“谁的、几条”（不混进行列表）。
    #[test]
    fn trash_snapshot_keeps_own_entries_and_counts_foreign() {
        let entries = vec![
            trash_entry(
                "t1",
                "dau.sql",
                ORIGIN_RESOURCES,
                "reports/dau.sql",
                TrashKind::File,
                2048,
            ),
            trash_entry(
                "t2",
                "draft.sql",
                "scratchpad",
                "draft.sql",
                TrashKind::File,
                10,
            ),
            trash_entry(
                "t3",
                "reports",
                "scratchpad",
                "reports",
                TrashKind::Folder,
                0,
            ),
        ];

        let snapshot = build_trash_snapshot(&entries);
        assert_eq!(snapshot.rows.len(), 1, "别人的条目不能混进行列表");
        assert_eq!(snapshot.rows[0].trash_id, "t1");
        assert_eq!(snapshot.rows[0].original_label, "resources/reports/dau.sql");
        assert_eq!(snapshot.rows[0].kind_label, "文件");
        assert_eq!(snapshot.rows[0].size_label, "2.0 KB");
        assert_eq!(snapshot.foreign, vec![("scratchpad".to_string(), 2)]);
    }

    /// 目录条目不给大小（要递归统计才有值，写 0 会被当成“空目录”）。
    #[test]
    fn trash_snapshot_leaves_folder_size_empty() {
        let entries = vec![trash_entry(
            "t1",
            "reports",
            ORIGIN_RESOURCES,
            "reports",
            TrashKind::Folder,
            0,
        )];
        let snapshot = build_trash_snapshot(&entries);
        assert_eq!(snapshot.rows[0].kind_label, "目录");
        assert!(snapshot.rows[0].size_label.is_empty());
        assert!(snapshot.foreign.is_empty());
    }

    #[test]
    fn repair_rows_group_in_order_and_keep_both_fingerprints() {
        let report = IndexScanReport {
            issues: vec![
                // 故意乱序给：输出应按分组 / 路径排好。
                IndexIssue::ContentChanged {
                    resource_id: "ar_2".to_string(),
                    name: "月报".to_string(),
                    rel_path: "reports/dau.sql".to_string(),
                    expected_hash: "1111111111111111".to_string(),
                    actual_hash: "2222222222222222".to_string(),
                },
                IndexIssue::UntrackedFile {
                    rel_path: "z/deep/外来表.sql".to_string(),
                },
                IndexIssue::MissingPayload {
                    resource_id: "ar_1".to_string(),
                    name: "周报".to_string(),
                    rel_path: "weekly.sql".to_string(),
                },
                IndexIssue::UntrackedFile {
                    rel_path: "a.sql".to_string(),
                },
            ],
        };

        let rows = build_repair_rows(&report);
        let groups: Vec<RepairGroup> = rows.iter().map(|row| row.group).collect();
        assert_eq!(
            groups,
            vec![
                RepairGroup::Untracked,
                RepairGroup::Untracked,
                RepairGroup::Missing,
                RepairGroup::Changed,
            ],
            "分组按原型顺序，组内按路径"
        );
        // 未登记行：标题取文件名（路径太长），完整路径在副文案里。
        assert_eq!(rows[0].title, "a.sql");
        assert!(rows[0].rel_path == "a.sql" && rows[0].resource_id.is_none());
        assert!(rows[0].detail.contains("resources/a.sql"));
        assert_eq!(rows[1].title, "外来表.sql", "嵌套路径取最后一段");
        // 指纹不匹配：两个指纹都缩到 12 位（期望 / 实际各一份，不藏任何一边）。
        assert_eq!(rows[3].hash_detail, "登记 111111111111 · 实际 222222222222");
        assert!(rows[3].detail.contains("reports/dau.sql"));
        assert_eq!(rows[3].resource_id.as_deref(), Some("ar_2"));
        assert!(rows[2].hash_detail.is_empty(), "缺本体没有指纹可对");
    }

    #[test]
    fn repair_rows_of_clean_report_are_empty() {
        assert!(build_repair_rows(&IndexScanReport::default()).is_empty());
    }

    #[test]
    fn size_formatting_stays_readable_at_every_scale() {
        assert_eq!(format_size(None), "");
        assert_eq!(format_size(Some(0)), "0 B");
        assert_eq!(format_size(Some(900)), "900 B");
        // 1 KB 以下不显示小数：`0.9 KB` 不如 `900 B` 直观。
        assert_eq!(format_size(Some(1023)), "1023 B");
        assert_eq!(format_size(Some(1229)), "1.2 KB");
        assert_eq!(format_size(Some(5 * 1024 * 1024)), "5.0 MB");
        assert_eq!(format_size(Some(-5)), "0 B", "负数按 0 处理，不显示负体积");
    }

    #[test]
    fn scale_uses_thousands_separator_and_tolerates_partial_info() {
        assert_eq!(format_scale(Some(12480), Some(18)), "12,480 行 × 18 列");
        assert_eq!(format_scale(Some(999), Some(3)), "999 行 × 3 列");
        assert_eq!(format_scale(Some(1000), None), "1,000 行");
        assert_eq!(format_scale(None, Some(7)), "7 列");
        assert_eq!(format_scale(None, None), "");
    }

    #[test]
    fn relative_time_buckets_and_clock_skew() {
        let now = Utc::now();
        assert_eq!(format_relative_time(now, now), "刚刚");
        assert_eq!(
            format_relative_time(now, now - Duration::minutes(5)),
            "5 分钟前"
        );
        assert_eq!(
            format_relative_time(now, now - Duration::hours(3)),
            "3 小时前"
        );
        assert_eq!(format_relative_time(now, now - Duration::days(2)), "2 天前");
        assert_eq!(
            format_relative_time(now, now - Duration::days(70)),
            "2 个月前"
        );
        assert_eq!(
            format_relative_time(now, now - Duration::days(800)),
            "2 年前"
        );
        // 时钟回拨不给"-5 分钟前"这种怪东西。
        assert_eq!(format_relative_time(now, now + Duration::hours(1)), "刚刚");
    }

    #[test]
    fn tail_follows_field_priority_per_kind() {
        let now = Utc::now();
        let file = row_model("ar_file", "file", Some(1229));
        assert_eq!(
            tail_for(&file, ArchiveKind::File, now),
            "1.2 KB · 刚刚",
            "文件型：大小 · 时间"
        );

        // 分析表型用规模替代大小。
        let mut analysis = row_model("ar_analysis", "analysis", None);
        analysis.row_count = Some(12480);
        analysis.column_count = Some(18);
        assert_eq!(
            tail_for(&analysis, ArchiveKind::Analysis, now),
            "12,480 行 × 18 列 · 刚刚"
        );

        // 引用型明说"无指纹"，不留空。
        let table_ref = row_model("ar_ref", "table_ref", None);
        assert_eq!(
            tail_for(&table_ref, ArchiveKind::TableRef, now),
            "无指纹 · 刚刚"
        );

        // 旧行（无体积）只剩时间，不留一个孤零零的分隔符。
        let legacy = row_model("ar_legacy", "file", None);
        assert_eq!(tail_for(&legacy, ArchiveKind::File, now), "刚刚");
    }

    #[test]
    fn details_carry_per_kind_size_and_real_history_count() {
        let now = Utc::now();
        let file = row_model("ar_file", "file", Some(1229));
        let analysis = {
            let mut row = row_model("ar_analysis", "analysis", None);
            row.row_count = Some(12480);
            row.column_count = Some(18);
            row
        };
        let table_ref = row_model("ar_ref", "table_ref", None);
        let resources = vec![file, analysis, table_ref];

        // 只有分析表有历史版本（版本数来自存储层的批量查询）。
        let mut counts = VersionCounts::new();
        counts.insert("ar_analysis".to_string(), 3);

        let snapshot = build_snapshot(SnapshotInputs {
            resources: &resources,
            global_rows: Vec::new(),
            global_resources: Vec::new(),
            statuses: &ArchiveStatuses::new(),
            history_counts: &counts,
            tags: empty_tags(),
            tag_dictionary: Vec::new(),
            folders: empty_folders(),
            group_dictionary: Vec::new(),
            previews: empty_previews(),
            read_only: false,
            now,
        });

        let file = snapshot.details.get("ar_file").expect("文件型详情");
        assert_eq!(file.size_label, "1.2 KB", "文件型给体积");
        assert_eq!(file.history_label, "", "无历史版本 → 空串（分区会被跳过）");
        assert!(file.readonly, "归档后只读");
        assert_eq!(file.content_hash.as_deref(), Some("abc"));
        // 详情用绝对时间（行上用相对时间）。
        assert_eq!(file.modified_label, format_timestamp(now));

        let analysis = snapshot.details.get("ar_analysis").expect("分析表详情");
        assert_eq!(analysis.size_label, "12,480 行 × 18 列");
        assert_eq!(analysis.history_label, "3 个历史版本");

        let table_ref = snapshot.details.get("ar_ref").expect("引用型详情");
        assert_eq!(table_ref.size_label, "", "引用型不给体积（不假装有值）");
    }

    /// 标签进快照：行带 **id + 名字**（筛选比 id、搜索比名字，一份数据两处用）、
    /// 详情带同一份 chip（同一装配来源）、顶层带词典（筛选菜单用）。
    #[test]
    fn snapshot_carries_tags_for_rows_details_and_dictionary() {
        let now = Utc::now();
        let resources = vec![
            row_model("ar_1", "file", Some(10)),
            row_model("ar_2", "file", Some(10)),
        ];
        let mut tags = ResourceTags::new();
        tags.insert(
            "ar_1".to_string(),
            vec![crate::models::AnalyticsTag {
                id: "at_1".to_string(),
                name: "报表".to_string(),
                color: None,
                icon: None,
                scope: "project".to_string(),
                created_at: now,
                deleted_at: None,
            }],
        );
        let dictionary = vec![TagOption {
            id: "at_1".to_string(),
            name: "报表".to_string(),
            count: 1,
        }];

        let snapshot = build_snapshot(SnapshotInputs {
            resources: &resources,
            global_rows: Vec::new(),
            global_resources: Vec::new(),
            statuses: &ArchiveStatuses::new(),
            history_counts: &VersionCounts::new(),
            tags: &tags,
            tag_dictionary: dictionary,
            folders: empty_folders(),
            group_dictionary: Vec::new(),
            previews: empty_previews(),
            read_only: false,
            now,
        });

        assert_eq!(snapshot.rows[0].tags, snapshot.details["ar_1"].tags);
        assert_eq!(snapshot.rows[0].tags[0].id, "at_1");
        assert_eq!(snapshot.rows[0].tags[0].name, "报表");
        assert!(
            snapshot.rows[1].tags.is_empty(),
            "没挂标签的行是空集，不是缺字段"
        );
        assert_eq!(snapshot.details["ar_1"].tags[0].name, "报表");
        assert!(snapshot.details["ar_2"].tags.is_empty());
        assert_eq!(snapshot.tags.len(), 1);
    }

    /// 搜索匹配面所需的字段（别名 / 来源表）在装配时就带上（原型 §2.2）。
    /// 行上不显示它们，但搜索要看：缺了这个装配，面板里搜别名就只会得到空列表。
    #[test]
    fn rows_carry_alias_and_source_table_for_the_search_surface() {
        let now = Utc::now();
        let mut alias = row_model("ar_1", "analysis", None);
        alias.alias = Some("月报".to_string());
        let mut table_ref = row_model("ar_2", "table_ref", None);
        table_ref.source_table = Some("dwd.dwd_orders".to_string());
        let resources = vec![alias, table_ref];

        let snapshot = build_snapshot(SnapshotInputs {
            resources: &resources,
            global_rows: Vec::new(),
            global_resources: Vec::new(),
            statuses: &ArchiveStatuses::new(),
            history_counts: &VersionCounts::new(),
            tags: empty_tags(),
            tag_dictionary: Vec::new(),
            folders: empty_folders(),
            group_dictionary: Vec::new(),
            previews: empty_previews(),
            read_only: false,
            now,
        });

        let haystack = crate::filter::search_haystack(&snapshot.rows[0]);
        assert!(haystack.contains("月报"), "别名进了匹配面：{haystack}");
        let haystack = crate::filter::search_haystack(&snapshot.rows[1]);
        assert!(
            haystack.contains("dwd.dwd_orders"),
            "来源表进了匹配面：{haystack}"
        );
    }

    /// 详情随行信息：**分组名**（从“按行归属 + 分组字典”拼）与**内容预览**都要进详情；
    /// 认不出的分组 id 当未分组（脏数据不能变出一行“分组：?”），没给预览的当仅元信息。
    #[test]
    fn details_carry_group_name_and_preview() {
        let now = Utc::now();
        let resources = vec![
            row_model("ar_1", "file", Some(10)),
            row_model("ar_2", "file", Some(10)),
        ];
        let mut folders = FolderMemberships::new();
        folders.insert("ar_1".to_string(), "af_1".to_string());
        folders.insert("ar_2".to_string(), "af_gone".to_string());
        let mut previews = PreviewMap::new();
        previews.insert(
            "ar_1".to_string(),
            crate::preview::Preview::Text {
                lines: vec!["select 1".to_string()],
                truncated: false,
            },
        );

        let snapshot = build_snapshot(SnapshotInputs {
            resources: &resources,
            global_rows: Vec::new(),
            global_resources: Vec::new(),
            statuses: &ArchiveStatuses::new(),
            history_counts: &VersionCounts::new(),
            tags: empty_tags(),
            tag_dictionary: Vec::new(),
            folders: &folders,
            group_dictionary: vec![GroupOption {
                id: "af_1".to_string(),
                name: "报表".to_string(),
            }],
            previews: &previews,
            read_only: false,
            now,
        });

        assert_eq!(snapshot.details["ar_1"].group.as_deref(), Some("报表"));
        assert!(
            snapshot.details["ar_2"].group.is_none(),
            "认不出的分组 id 当未分组"
        );
        assert_eq!(
            snapshot.details["ar_1"].preview.lines(),
            ["select 1".to_string()]
        );
        assert_eq!(
            snapshot.details["ar_2"].preview.note(),
            Some(crate::preview::ONLY_META_NOTE),
            "宿主没给预览 → 仅元信息（不假装有内容）"
        );
    }

    #[test]
    fn timestamps_are_absolute_and_zero_padded() {
        let when = DateTime::parse_from_rfc3339("2026-09-16T04:05:00Z")
            .expect("rfc3339")
            .with_timezone(&Utc);
        assert_eq!(format_timestamp(when), "2026-09-16 04:05");
    }

    #[test]
    fn snapshot_counts_route_status_before_kind() {
        let now = Utc::now();
        let resources = vec![
            row_model("ar_1", "file", Some(1024)),
            row_model("ar_2", "analysis", None),
            row_model("ar_3", "table_ref", None),
            row_model("ar_4", "file", Some(10)),
            row_model("ar_5", "file", Some(10)),
        ];
        let mut statuses = ArchiveStatuses::new();
        // 异常态压过 kind：这两个不进各自 kind 的桶，而进异常桶。
        statuses.insert("ar_4".to_string(), ArchiveStatus::Missing);
        statuses.insert("ar_5".to_string(), ArchiveStatus::ContentChanged);

        let snapshot = build_snapshot(SnapshotInputs {
            resources: &resources,
            global_rows: Vec::new(),
            global_resources: Vec::new(),
            statuses: &statuses,
            history_counts: &VersionCounts::new(),
            tags: empty_tags(),
            tag_dictionary: Vec::new(),
            folders: empty_folders(),
            group_dictionary: Vec::new(),
            previews: empty_previews(),
            read_only: true,
            now,
        });

        assert_eq!(snapshot.rows.len(), 5);
        assert_eq!(snapshot.counts.total, 5);
        assert_eq!(snapshot.counts.archived, 1);
        assert_eq!(snapshot.counts.analysis, 1);
        assert_eq!(snapshot.counts.table_ref, 1);
        assert_eq!(snapshot.counts.missing, 1);
        assert_eq!(snapshot.counts.drifted, 1);
        assert!(snapshot.counts.has_issues());
        assert!(snapshot.read_only);
        // 行状态与计数同源（同一个 statuses 映射）。
        assert_eq!(snapshot.rows[3].status, ArchiveStatus::Missing);
    }

    /// 历史行：快照里带上当时的大小与指纹（生产路径就是资源记录的序列化）。
    fn history_version(version: i32, size: i32, hash: &str, at: DateTime<Utc>) -> ResourceVersion {
        let mut record = row_model("ar_1", "file", Some(size));
        record.version = version;
        record.content_hash = Some(hash.to_string());
        ResourceVersion {
            id: format!("arv_{version}"),
            resource_id: "ar_1".to_string(),
            version,
            snapshot: serde_json::to_value(&record).expect("snapshot json"),
            created_at: at,
        }
    }

    #[test]
    fn version_rows_include_current_and_compare_adjacent_versions() {
        let now = Utc::now();
        let mut current = row_model("ar_1", "file", Some(1024));
        current.version = 3;
        current.content_hash = Some("333333333333ffff".to_string());
        let history = vec![
            history_version(2, 2048, "222222222222ffff", now),
            history_version(1, 1024, "111111111111ffff", now),
        ];

        // 只有 v2 还留着副本：v1 的已被保留策略裁掉。
        let rows = build_version_rows(&current, &history, &[2], true);
        let versions: Vec<i32> = rows.iter().map(|row| row.version).collect();
        assert_eq!(versions, vec![3, 2, 1], "降序，且当前版本也在列表里");
        assert!(rows[0].is_current);
        assert!(!rows[1].is_current);
        assert!(rows[0].has_copy, "当前版本的“副本”就是本体");
        assert!(rows[1].has_copy);
        assert!(!rows[2].has_copy, "副本被裁的版本要露出来");
        assert_eq!(rows[0].hash_short, "333333333333", "指纹只展示前 12 位");
        assert_eq!(rows[0].delta_label, "较 v2 大小-1.0 KB · 指纹已变");
        assert_eq!(rows[1].delta_label, "较 v1 大小+1.0 KB · 指纹已变");
        assert!(rows[2].delta_label.is_empty(), "最旧一版没有可比的上一版");
        assert_eq!(rows[0].time_label, format_timestamp(now));
    }

    #[test]
    fn version_rows_tolerate_unreadable_snapshots() {
        let mut current = row_model("ar_1", "file", Some(1024));
        current.version = 2;
        let broken = ResourceVersion {
            id: "arv_1".to_string(),
            resource_id: "ar_1".to_string(),
            version: 1,
            snapshot: Value::Null,
            created_at: Utc::now(),
        };

        let rows = build_version_rows(&current, &[broken], &[], false);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[1].hash_short, "—", "解析不出来的快照不伪造指纹");
        assert_eq!(rows[1].size_label, "");
        assert!(
            rows[1].delta_label.is_empty(),
            "两侧都没值就不给差异（宁可不比也不编）"
        );
    }

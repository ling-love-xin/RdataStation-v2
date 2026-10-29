//! `grid/` 全量单元测试（2026-10-29 拆分时自 `crate::grid` 末尾 `mod tests` 整体迁入）。
//! 安全模式：不通配导入（`#[gpui_kit::test]` 与 `use gpui_kit::*` 会自相残杀）。

use crate::grid::{
    CELL_DISPLAY_LIMIT, MAX_SINGLE_SEGMENT_ROWS, ContextAction, ContextTarget, GridSelection,
    ResultGridDelegate, ResultMatch, ResultStatus, ResultToolbar, WINDOW_FETCH_NOTICE,
    column_distinct_values, column_stats, column_stats_from_values, context_menu_items,
    duration_text, fetch_more_hint, group_by_values, column_menu_items,
    column_browser_menu_items, format_cell_text,
    stats_lines, status_action_placeholders,
    status_segments, thousands, truncation_notice, truncated_hint, validate_edit_value,
    window_fetch_active,
};
use crate::grid::status::{FIRST_PAGER_ACTION, FIRST_SYSTEM_ACTION};
use crate::grid::status::format_integer_cell;
use crate::grid::cell::{
    blob_summary, cell_display_text, cell_style, clip_cell_display, pretty_json_display,
    classify_value, detect_image_format, hex_lines, json_tree_items, ViewerMode,
};
use crate::grid::header::header_tooltip_lines;
use crate::grid::stats::fmt_stat_num;
use crate::grid::text::{cell_matches, find_in_cell, preview_of};
use crate::grid::view_rows::compare_cells;
use gpui_kit::TextAlign;
use gpui_kit::assets::IconName;
use workbench_shell::model::ColumnAnnotation;
use workbench_shell::result_style::{AlignPref, ColorRolePref, TypeStylePrefs};
use crate::rowset::{LocalRow, LocalRowSet, RowRef};
use crate::store::{ColumnKind, ColumnType};
use gpui_kit::App;
use std::rc::Rc;
use gpui_kit::component::table::{ColumnSort, TableDelegate as _};
use ::shared::{CurrentTier, FactFrame};
use arrow::array::{ArrayRef, Float64Array, Int64Array, StringArray};
use arrow::datatypes::{Field, Schema};
use arrow::record_batch::RecordBatch;
use std::sync::Arc;

    /// 【B5】由同构数组构造内存档事实帧（测试辅助；列名与数组顺序一一对应）。
    fn fact_frame(columns: &[&str], arrays: Vec<ArrayRef>) -> Arc<FactFrame> {
        let fields: Vec<Field> = columns
            .iter()
            .zip(arrays.iter())
            .map(|(name, array)| Field::new(*name, array.data_type().clone(), true))
            .collect();
        let schema = Arc::new(Schema::new(fields));
        let batch = RecordBatch::try_new(schema, arrays).expect("构造 RecordBatch");
        Arc::new(
            FactFrame::new(
                columns.iter().map(|n| n.to_string()).collect(),
                vec![batch],
            )
            .expect("构造事实档"),
        )
    }

    fn grid() -> ResultGridDelegate {
        let mut grid = ResultGridDelegate::empty("尚未执行");
        grid.set_data(
            vec!["id".to_string(), "name".to_string()],
            vec![
                vec!["1".to_string(), "a".to_string()],
                vec!["2".to_string(), "NULL".to_string()],
            ],
        );
        grid
    }

    /// 造一份指定行的网格（B15 的筛选/排序测试用：列固定为 id / name）
    fn grid_with(rows: &[&[&str]]) -> ResultGridDelegate {
        let mut grid = ResultGridDelegate::empty("尚未执行");
        grid.set_data(
            vec!["id".to_string(), "name".to_string()],
            rows.iter()
                .map(|row| row.iter().map(|cell| cell.to_string()).collect())
                .collect(),
        );
        grid
    }

    /// 右键落点（用例只改关心的那几项）
    fn target(value: &str, name: &str, column: usize) -> ContextTarget {
        ContextTarget {
            value: value.to_string(),
            column_name: name.to_string(),
            column,
            row: 0,
            rows: 2,
            frozen: false,
            hidden_columns: 0,
            insight: false,
            table: None,
            column_filter_active: false,
            column_values: Vec::new(),
            sort_active: false,
            row_ref: RowRef::default(),
            // 默认只读：既有菜单断言不摆行级三项；可编辑用例在测试里显式覆盖
            editable: false,
            is_deleted: false,
        }
    }

    /// 带表名的右键落点（【A9】INSERT 项只在表名猜得到时出现）
    fn target_with_table(value: &str, name: &str, column: usize) -> ContextTarget {
        let mut target = target(value, name, column);
        target.table = Some("orders".to_string());
        target
    }

    /// 【P1】表头悬停分块：无注释 = 单行（与今天一致）；有注释 = 标题 + 元数据行
    #[test]
    fn header_tooltip_lines_build_title_and_meta_blocks() {
        // 无注释：单行「列名 · 类型标签」；无类型 = 只列名
        let (title, meta) = header_tooltip_lines("total", Some("numeric"), None);
        assert_eq!(title, "total · numeric");
        assert!(meta.is_empty());
        let (title, meta) = header_tooltip_lines("id", None, None);
        assert_eq!(title, "id");
        assert!(meta.is_empty());

        // 有注释：标题 + 来源 / 注释 / 类型 / 可空 / 默认（缺的字段不摆那一行）
        let ann = ColumnAnnotation {
            source_table: Some("public.orders".into()),
            source_column: Some("total".into()),
            comment: Some("总额".into()),
            data_type: Some("numeric".into()),
            nullable: Some(true),
            default_value: Some("0".into()),
        };
        let (title, meta) = header_tooltip_lines("total", Some("numeric"), Some(&ann));
        assert_eq!(title, "total · numeric");
        assert_eq!(
            meta,
            vec![
                "来源：public.orders.total",
                "注释：总额",
                "类型：numeric",
                "可空：是",
                "默认：0",
            ]
        );

        // 表达式列（来源两处 None）：标题在，元数据只剩注释 / 可空等
        let ann = ColumnAnnotation {
            source_table: None,
            source_column: None,
            comment: Some("合计".into()),
            data_type: None,
            nullable: None,
            default_value: None,
        };
        let (title, meta) = header_tooltip_lines("cnt", None, Some(&ann));
        assert_eq!(title, "cnt");
        assert_eq!(meta, vec!["注释：合计"]);
    }

    // ========== 值查看器分派（classify / 图片魔数 / hex 转储 / JSON 树） ==========

    /// 值查看器分派：Binary → 图片魔数命中 Image / 否则 Hex；文本形态 `{` / `[` → Json
    #[test]
    fn classify_value_dispatches_by_kind_and_shape() {
        // Binary：PNG 魔数 → Image（格式 + 字节数 = hex 长度 / 2）
        assert_eq!(
            classify_value(ColumnKind::Binary, "89504E470D0A1A0A"),
            ViewerMode::Image {
                format: "PNG",
                size: 8,
            },
        );
        // Binary：非图片 → Hex
        assert_eq!(
            classify_value(ColumnKind::Binary, "0102030405"),
            ViewerMode::Hex,
        );
        // 空 Binary → Hex（无魔数）
        assert_eq!(classify_value(ColumnKind::Binary, ""), ViewerMode::Hex);
        // Json 列对象 / 数组 → Json
        assert_eq!(
            classify_value(ColumnKind::Json, "{\"a\":1}"),
            ViewerMode::Json
        );
        assert_eq!(classify_value(ColumnKind::Json, "[1,2]"), ViewerMode::Json);
        // Json 列标量 → Text（没有树可展）
        assert_eq!(classify_value(ColumnKind::Json, "42"), ViewerMode::Text);
        // 文本列存 JSON 也按值形态走（`{` 前缀）
        assert_eq!(
            classify_value(ColumnKind::Text, "{\"a\":1}"),
            ViewerMode::Json,
        );
        // 数值 / 普通文本 → Text
        assert_eq!(classify_value(ColumnKind::Integer, "42"), ViewerMode::Text);
        assert_eq!(classify_value(ColumnKind::Text, "hello"), ViewerMode::Text);
        // 行号槽 / 转置（Unknown）也按值形态走
        assert_eq!(classify_value(ColumnKind::Unknown, "[1]"), ViewerMode::Json);
    }

    /// 图片魔数：PNG / JPEG / GIF / WebP（大小写不敏感），非魔数 → None
    #[test]
    fn detect_image_format_recognizes_magic_bytes() {
        assert_eq!(detect_image_format("89504e470d0a1a0a"), Some("PNG"));
        assert_eq!(detect_image_format("89504E47"), Some("PNG")); // 大写同样命中
        assert_eq!(detect_image_format("ffd8ffe000104a46"), Some("JPEG"));
        assert_eq!(detect_image_format("474946383961"), Some("GIF"));
        assert_eq!(detect_image_format("524946460000000057454250"), Some("WebP"));
        assert_eq!(detect_image_format("00000000"), None);
        assert_eq!(detect_image_format(""), None);
        // 非 hex 文本不误判
        assert_eq!(detect_image_format("hello world"), None);
    }

    /// hex 转储：offset 行格式 / limit 截断 / ASCII 可读字符 / 越界空
    #[test]
    fn hex_lines_paginates_and_formats() {
        let bytes = b"Hello, World!";
        let lines = hex_lines(bytes, 0, 1);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].starts_with("[00000000] "));
        assert!(lines[0].contains("48 65 6c 6c 6f")); // "Hello"
        assert!(lines[0].ends_with("|Hello, World!")); // ASCII 栏可读
        // limit 大于可用行 → 只给实际行数
        assert_eq!(hex_lines(bytes, 0, 99).len(), 1);
        // offset 越界 → 空
        assert!(hex_lines(bytes, 999, 5).is_empty());
        // 13 字节只有一页：offset 16 起越界
        assert!(hex_lines(bytes, 16, 1).is_empty());
        // 二进制不可读字节 → '.'
        let bin = [0x00u8, 0x01, 0x41, 0xff];
        let l = &hex_lines(&bin, 0, 1)[0];
        assert!(l.contains("00 01 41 ff"));
        assert!(l.ends_with("|..A."));
    }

    /// JSON → 嵌套树：对象 / 数组 / 标量标签、路径 id、预展开、截断占位
    #[test]
    fn json_tree_items_builds_nested_tree() {
        let items =
            json_tree_items(r#"{"user":{"name":"a","tags":["x","y"]},"n":1}"#).expect("合法 JSON 应建树");
        assert_eq!(items.len(), 1);
        let root = &items[0];
        assert_eq!(root.id, "$");
        assert!(root.label.contains("对象"));
        assert!(root.is_folder());
        assert!(root.is_expanded()); // 顶层预展开
        // 两个直接子项：user 对象 + n 标量
        assert_eq!(root.children.len(), 2);
        let user = root
            .children
            .iter()
            .find(|c| c.id == "$.user")
            .expect("user 子项");
        assert!(user.is_folder());
        assert!(user.is_expanded()); // 深度 1 也预展开
        assert_eq!(user.children.len(), 2);
        let tags = user
            .children
            .iter()
            .find(|c| c.id == "$.user.tags")
            .expect("tags 子项");
        assert!(tags.is_folder());
        assert!(!tags.is_expanded()); // 深度 2 收起
        assert_eq!(tags.children.len(), 2);
        assert_eq!(tags.children[0].id, "$.user.tags[0]");
        assert_eq!(tags.children[0].label, "0: \"x\"");
        let n = root.children.iter().find(|c| c.id == "$.n").expect("n 子项");
        assert!(!n.is_folder()); // 标量叶子
        assert_eq!(n.label, "n: 1");
    }

    /// JSON 树：超大数组截断 + 非法 JSON → None
    #[test]
    fn json_tree_items_truncates_large_arrays_and_rejects_non_json() {
        // 超过 JSON_TREE_CHILD_LIMIT 的数组 → 截断 + 「… 还有 N 项」占位
        let big = format!(
            "[{}]",
            (0..700).map(|i| i.to_string()).collect::<Vec<_>>().join(",")
        );
        let items = json_tree_items(&big).expect("大数组应建树");
        let root = &items[0];
        assert_eq!(root.children.len(), crate::grid::cell::JSON_TREE_CHILD_LIMIT + 1);
        let last = root.children.last().expect("占位节点");
        assert!(last.label.starts_with("… 还有 "));
        // 非法 JSON → None
        assert!(json_tree_items("not json").is_none());
        assert!(json_tree_items("{broken").is_none());
        assert!(json_tree_items("").is_none());
    }

    /// 【B14】右键菜单的项：按值筛选 / 复制（此值 · 此列 · 整行）/ 冻结（文案随冻结状态变）
    #[test]
    fn context_menu_offers_filter_copy_and_freeze() {
        let target = target("orders", "name", 1);
        let items = context_menu_items(&target);
        let labels: Vec<&str> = items.iter().map(|item| item.label.as_str()).collect();
        assert_eq!(
            labels,
            [
                "查看完整值…",
                "按值筛选「orders」",
                "复制此值",
                "复制此值为 JSON",
                "复制此列（2 个取值）",
                "复制整行（TSV）",
                "复制整行为 JSON",
                "按「name」升序（下发源库）",
                "按「name」降序（下发源库）",
                "冻结「name」",
                "隐藏「name」",
                "列统计「name」",
            ]
        );
        assert_eq!(
            items[0].action,
            ContextAction::PreviewValue("orders".to_string()),
            "【遗留 #7】查看完整值：值原样给面板（弹层是宿主的事）"
        );
        assert_eq!(
            items[1].action,
            ContextAction::FilterByValue("orders".to_string()),
            "值原样给面板（预览只管显示）"
        );
        assert_eq!(items[2].action, ContextAction::CopyValue("orders".to_string()));
        assert_eq!(
            items[3].action,
            ContextAction::CopyValueJson("orders".to_string()),
            "【A9】值 JSON 与「复制此值」平行（NULL → null，其余字符串）"
        );
        assert_eq!(
            items[4].action,
            ContextAction::CopyColumn(1),
            "复制整列认的是**数据列**下标（1 列、行号槽已折算掉）"
        );
        assert_eq!(
            items[5].action,
            ContextAction::CopyRow(0),
            "复制整行认的是**视图行**下标，不是列下标（两个都是数字，传反了菜单照样能摆出来）"
        );
        assert_eq!(
            items[6].action,
            ContextAction::CopyRowJson(0),
            "【A9】整行 JSON（单对象，与导出 JSON 档同口径）"
        );
        assert_eq!(
            items[7].action,
            ContextAction::SortDown {
                column: "name".to_string(),
                descending: false
            }
        );
        assert_eq!(
            items[8].action,
            ContextAction::SortDown {
                column: "name".to_string(),
                descending: true
            }
        );
        assert!(
            items[7].separator_before,
            "“对整份结果的操作”那组前面要有分隔线"
        );
        assert!(!items[9].separator_before, "冻结与排序同组");
        assert_eq!(items[9].action, ContextAction::ToggleFreeze(1));
        assert_eq!(
            items[10].action,
            ContextAction::HideColumn(1),
            "【P2】隐藏此列跟在冻结后（都是列级动作）"
        );
        assert_eq!(
            items[11].action,
            ContextAction::ColumnStats { column: 1 },
            "【P2-3】列统计与隐藏同组"
        );
        assert!(
            !items
                .iter()
                .any(|item| item.action == ContextAction::ShowAllColumns),
            "没有隐藏列时不摆「显示全部列」"
        );

        let mut done = target.clone();
        done.frozen = true;
        let items = context_menu_items(&done);
        assert_eq!(items[9].label, "取消冻结此列", "已冻结时给的是反向动作");

        // 【P2】隐藏了列才出现「显示全部列」
        let mut hidden = target.clone();
        hidden.hidden_columns = 2;
        let items = context_menu_items(&hidden);
        assert_eq!(
            items[12].action,
            ContextAction::ShowAllColumns,
            "隐藏非空时给恢复入口（排在「列统计」之后）"
        );
    }

    /// 【A9】「复制整行为 INSERT」只在表名猜得到时出现（面板从来源 SQL 猜好传来）：
    /// 猜不到宁可少一项，也不给一条插到错误表的语句。
    #[test]
    fn context_menu_offers_insert_only_when_table_is_known() {
        let without = context_menu_items(&target("orders", "name", 1));
        assert!(
            !without
                .iter()
                .any(|item| matches!(item.action, ContextAction::CopyRowInsert(_))),
            "表名未知时不摆 INSERT 项"
        );
        let with = context_menu_items(&target_with_table("orders", "name", 1));
        let insert = with
            .iter()
            .find(|item| matches!(item.action, ContextAction::CopyRowInsert(_)))
            .expect("表名已知时要摆 INSERT 项");
        assert_eq!(insert.label, "复制整行为 INSERT（orders）", "表名写进文案");
        assert!(!insert.separator_before, "与复制组同组（紧跟整行 JSON 后）");
    }

    /// 【#28-B3】行级动作（新增 / 复制 / 删除一行）的菜单闸口：
    /// - 只读结果集（`!editable`）整组不摆；
    /// - 已标记删除的底层行三项全不摆；
    /// - 可编辑且未删时三项都摆，独立分组（新增项带分隔线），排在复制段之后、排序段之前；
    /// - 动作携带视图行号（与复制整行同口径，不是列下标）。
    #[test]
    fn context_menu_row_actions_gate_on_editable_and_deleted() {
        // 只读：三项都不摆
        let read_only = context_menu_items(&target("orders", "name", 1));
        assert!(
            !read_only
                .iter()
                .any(|item| matches!(
                    item.action,
                    ContextAction::InsertRowBelow(_)
                        | ContextAction::DuplicateRow(_)
                        | ContextAction::DeleteRow(_)
                )),
            "只读结果集不摆行级动作"
        );

        // 可编辑、未删：三项都摆
        let mut editable = target("orders", "name", 1);
        editable.editable = true;
        let items = context_menu_items(&editable);
        let insert = items
            .iter()
            .find(|item| matches!(item.action, ContextAction::InsertRowBelow(_)))
            .expect("可编辑结果集要摆新增行");
        let duplicate = items
            .iter()
            .find(|item| matches!(item.action, ContextAction::DuplicateRow(_)))
            .expect("可编辑结果集要摆复制行");
        let delete = items
            .iter()
            .find(|item| matches!(item.action, ContextAction::DeleteRow(_)))
            .expect("可编辑结果集要摆删除行");
        assert_eq!(insert.action, ContextAction::InsertRowBelow(0));
        assert_eq!(duplicate.action, ContextAction::DuplicateRow(0));
        assert_eq!(delete.action, ContextAction::DeleteRow(0));
        assert!(insert.separator_before, "行级动作独立分组，新增项前带分隔线");
        assert!(!duplicate.separator_before, "复制 / 删除与新增同组");
        assert!(!delete.separator_before, "复制 / 删除与新增同组");

        // 分组位置：复制段（CopyRowJson / CopyRowInsert）之后、排序段（SortDown）之前
        let row_action_indexes = [insert, duplicate, delete]
            .iter()
            .map(|item| items.iter().position(|candidate| candidate.action == item.action))
            .collect::<Option<Vec<_>>>()
            .expect("三项都在菜单里");
        let first_row_action = *row_action_indexes.iter().min().unwrap();
        let last_row_action = *row_action_indexes.iter().max().unwrap();
        let last_copy = items
            .iter()
            .rposition(|item| {
                matches!(
                    item.action,
                    ContextAction::CopyRow(_)
                        | ContextAction::CopyRowJson(_)
                        | ContextAction::CopyRowInsert(_)
                )
            })
            .expect("复制段至少有一项");
        let first_sort = items
            .iter()
            .position(|item| matches!(item.action, ContextAction::SortDown { .. }))
            .expect("排序段至少有一项");
        assert!(
            last_copy < first_row_action,
            "行级动作排在复制段之后"
        );
        assert!(
            last_row_action < first_sort,
            "行级动作排在排序段之前"
        );

        // 已删除：三项全不摆
        let mut deleted = target("orders", "name", 1);
        deleted.editable = true;
        deleted.is_deleted = true;
        let deleted_items = context_menu_items(&deleted);
        assert!(
            !deleted_items
                .iter()
                .any(|item| matches!(
                    item.action,
                    ContextAction::InsertRowBelow(_)
                        | ContextAction::DuplicateRow(_)
                        | ContextAction::DeleteRow(_)
                )),
            "已删除行不再摆行级动作"
        );
    }

    /// 【M8】「洞察此列」只在**能洞察**时出现（宿主接了端口 + 这份结果可取样）：
    /// 不能洞察时连项都不摆（能力没有就不给入口，与「按值筛选」同口径）。
    #[test]
    fn context_menu_only_offers_insight_when_available() {
        let without = context_menu_items(&target("orders", "name", 1));
        assert!(
            !without
                .iter()
                .any(|item| matches!(item.action, ContextAction::InsightColumn { .. })),
            "不能洞察时不该摆这一项：{without:?}"
        );

        let mut with = target("orders", "name", 1);
        with.insight = true;
        let with = context_menu_items(&with);
        let last = with.last().expect("至少有一项");
        assert_eq!(last.label, "洞察「name」这一列");
        assert_eq!(
            last.action,
            ContextAction::InsightColumn {
                column: "name".to_string()
            }
        );
        assert!(last.separator_before, "它自成一段，前面要有分隔线");
    }

    /// 值预览：空白折叠 + 截断（长值不该把菜单撑开）
    #[test]
    fn the_value_preview_folds_and_truncates() {
        assert_eq!(preview_of("a\nb  c"), "a b c");
        let long = "x".repeat(40);
        let preview = preview_of(&long);
        assert_eq!(preview.chars().count(), 25, "24 个字符 + 省略号");
        assert!(preview.ends_with('…'), "{preview}");
        let items = context_menu_items(&target(&long, "c", 0));
        assert!(items[0].label.contains('…'), "{}", items[0].label);
    }

    /// 复制整列 / 整行：走**视图行序**，并按 TSV 规则转义（与 `ResultEntry::to_tsv`
    /// 同一份实现）：带制表符 / 换行 / 引号的值不许把粘出去的形状搞坏。
    #[test]
    fn copying_a_column_and_a_row_keeps_the_tsv_shape() {
        let mut grid = grid_with(&[&["1", "plain"], &["2", "two\tcells"], &["3", "say \"hi\""]]);

        // 整行：列之间是制表符，只有会破坏形状的格子才加引号
        assert_eq!(grid.row_text(0), "1\tplain");
        assert_eq!(grid.row_text(1), "2\t\"two\tcells\"");
        assert_eq!(grid.row_text(2), "3\t\"say \"\"hi\"\"\"");
        assert_eq!(
            grid.row_text(99),
            "",
            "行不存在给空串（不摆这一项的是界面）"
        );

        // 整列：一行一个取值（本身就是单列 TSV）
        assert_eq!(grid.column_text(0), "1\n2\n3");
        assert_eq!(
            grid.column_text(1),
            "plain\n\"two\tcells\"\n\"say \"\"hi\"\"\""
        );

        // 取值里的换行也被包起来：不然它会拆出一行，看上去像多了一行数据
        let with_break = grid_with(&[&["1", "line\nbreak"]]);
        assert_eq!(with_break.column_text(1), "\"line\nbreak\"");
        assert_eq!(with_break.row_text(0), "1\t\"line\nbreak\"");

        // 视图行序：本地排序 / 筛选之后，复制的是**看着的那些行**
        grid.apply_sort(Some((0, true)));
        assert_eq!(grid.column_text(0), "3\n2\n1");
        assert_eq!(grid.row_text(0), "3\t\"say \"\"hi\"\"\"");

        grid.set_filter("two");
        assert_eq!(grid.column_text(0), "2", "筛选生效时只复制看得见的行");
    }

    /// 【A9】整行 JSON / INSERT：与 TSV 同一份视图行序口径；JSON 走导出 JSON 的 NULL /
    /// 字符串规则，INSERT 走导出字面量规则；表名没同步时 INSERT 给空串（菜单不摆，防御）。
    #[test]
    fn row_json_and_insert_follow_the_view_order() {
        let mut grid = grid_with(&[&["1", "a"], &["2", "NULL"]]);
        // JSON：NULL → null；列名与值都是字符串
        assert_eq!(
            grid.row_json_text(0),
            "{\n  \"id\": \"1\",\n  \"name\": \"a\"\n}"
        );
        assert_eq!(
            grid.row_json_text(1),
            "{\n  \"id\": \"2\",\n  \"name\": null\n}"
        );
        // 没同步表名 → INSERT 空串（界面已不摆那一项，这里是防御）
        assert_eq!(grid.row_insert_text(0), "");
        // 同步表名后：值字面量与导出档同口径（NULL 不加引号）
        grid.set_result_table(Some("orders".to_string()));
        assert_eq!(
            grid.row_insert_text(1),
            "INSERT INTO orders (id, name) VALUES (2, NULL);"
        );
        // 视图行序：排序后复制的是看着的那行
        grid.apply_sort(Some((0, true)));
        assert_eq!(grid.row_json_text(0), "{\n  \"id\": \"2\",\n  \"name\": null\n}");
        // 换数据（set_data）→ 表名跟着清（防旧表名插到新结果上）
        grid.set_data(vec!["x".to_string()], vec![vec!["y".to_string()]]);
        assert_eq!(grid.row_insert_text(0), "");
    }

    /// 【B15】冻结列：切换、换数据时清空（列都换了一批，旧列号没意义）
    #[test]
    fn freezing_columns_toggles_and_resets_with_data() {
        let mut grid = grid();
        assert!(!grid.is_frozen(0));
        assert!(grid.toggle_freeze(0), "第一次是冻结");
        assert!(grid.is_frozen(0));
        assert!(grid.toggle_freeze(1));
        assert!(!grid.toggle_freeze(0), "再切一次是取消");
        assert!(!grid.is_frozen(0) && grid.is_frozen(1));

        grid.set_data(
            vec!["a".to_string()],
            vec![vec!["1".to_string()]],
        );
        assert!(!grid.is_frozen(1), "换了一份结果：冻结不跟过来");
    }

    /// 【B15】筛选只改视图行序：命中那些行还在原位，行号跟的是视图
    #[test]
    fn filtering_keeps_matching_rows_in_place() {
        let mut grid = grid_with(&[&["1", "orders"], &["2", "users"], &["3", "orders_archive"]]);
        grid.set_filter("orders");
        assert_eq!(grid.visible_row_count(), 2, "命中两行");
        assert_eq!(grid.data_row_count(), 3, "数据一行没动（清掉筛选就能原样回来）");
        assert_eq!(grid.cell(0, 1), "1", "首列显示的是视图行号");
        assert_eq!(grid.cell(0, 2), "orders");
        assert_eq!(grid.cell(1, 2), "orders_archive");
        assert_eq!(grid.cell(2, 2), "", "越界的视图行给空串（不 panic）");

        // 大小写不敏感（与 DBeaver 的本地筛选同类）
        grid.set_filter("ORDERS");
        assert_eq!(grid.visible_row_count(), 2);

        // 清除筛选：全部回来，顺序不变
        grid.set_filter("");
        assert_eq!(grid.visible_row_count(), 3);
        assert_eq!(grid.visible_rows()[0][1], "orders");
        assert_eq!(grid.visible_rows()[2][1], "orders_archive");
    }

    /// 【B15】排序：数字按数值比（`1, 2, 10` 不该排成 `1, 10, 2`），字符串按字典序
    #[test]
    fn sorting_is_numeric_when_the_values_look_numeric() {
        assert_eq!(
            compare_cells(Some(&"2".to_string()), Some(&"10".to_string())),
            std::cmp::Ordering::Less
        );
        assert_eq!(
            compare_cells(Some(&"b".to_string()), Some(&"a".to_string())),
            std::cmp::Ordering::Greater
        );
        assert_eq!(
            compare_cells(None, Some(&"1".to_string())),
            std::cmp::Ordering::Greater,
            "缺列排在最后"
        );

        let mut grid = grid_with(&[&["10", "b"], &["2", "c"], &["1", "a"]]);
        grid.apply_sort(Some((0, false)));
        assert_eq!(
            grid.visible_rows()
                .iter()
                .map(|row| row[0].clone())
                .collect::<Vec<_>>(),
            ["1", "2", "10"],
            "升序按数值"
        );
        grid.apply_sort(Some((1, true)));
        assert_eq!(
            grid.visible_rows()
                .iter()
                .map(|row| row[1].clone())
                .collect::<Vec<_>>(),
            ["c", "b", "a"],
            "降序按字符串"
        );
        grid.apply_sort(None);
        assert_eq!(
            grid.visible_rows()
                .iter()
                .map(|row| row[0].clone())
                .collect::<Vec<_>>(),
            ["10", "2", "1"],
            "取消排序回原顺序（不是部分退还）"
        );
    }

    /// 【B15】筛选与排序叠在一起：先筛后排，两者都只动视图
    #[test]
    fn filter_and_sort_stack_on_the_same_view() {
        let mut grid = grid_with(&[&["10", "orders"], &["2", "orders"], &["7", "users"]]);
        grid.set_filter("orders");
        grid.apply_sort(Some((0, false)));
        assert_eq!(grid.visible_row_count(), 2);
        assert_eq!(grid.visible_rows()[0][0], "2");
        assert_eq!(grid.visible_rows()[1][0], "10");
    }

    /// 类型 → 格子样式（表驱动）：数字右对齐、布尔 / 时间族等宽、**未知档就是今天的样式**
    ///
    /// 这条钉的是“类型感知渲染”的最后一跳：模型说“是数字”还不够，渲染得真按右对齐画。
    /// 默认偏好（`TypeStylePrefs::default()`）下与历史逐项一致：对齐全继承类型默认、不染色。
    #[test]
    fn the_cell_style_follows_the_column_type_and_unknown_stays_as_today() {
        let prefs = TypeStylePrefs::default();
        let cases: &[(ColumnKind, TextAlign, bool)] = &[
            (ColumnKind::Integer, TextAlign::Right, false),
            (ColumnKind::Float, TextAlign::Right, false),
            (ColumnKind::Decimal, TextAlign::Right, false),
            (ColumnKind::Boolean, TextAlign::Left, true),
            (ColumnKind::Timestamp, TextAlign::Left, true),
            (ColumnKind::Date, TextAlign::Left, true),
            (ColumnKind::Time, TextAlign::Left, true),
            (ColumnKind::Uuid, TextAlign::Left, true),
            (ColumnKind::Text, TextAlign::Left, false),
            (ColumnKind::Json, TextAlign::Left, false),
            (ColumnKind::Binary, TextAlign::Left, false),
            // 没类型 / 认不出：左对齐 + 默认字体 —— 就是今天的样子
            (ColumnKind::Unknown, TextAlign::Left, false),
        ];
        for (kind, align, monospace) in cases {
            let style = cell_style(*kind, &prefs);
            assert_eq!(style.align, *align, "{kind:?} 的对齐");
            assert_eq!(style.monospace, *monospace, "{kind:?} 的字体");
            assert_eq!(style.color_role, ColorRolePref::Inherit, "{kind:?} 默认不染色");
        }
    }

    /// 【类型样式】按族对齐覆盖：显式对齐覆盖类型默认；`Inherit` 保持原默认。
    #[test]
    fn the_cell_style_honors_per_family_align_overrides() {
        let prefs = TypeStylePrefs {
            align_numeric: AlignPref::Left,
            align_text: AlignPref::Center,
            align_bool: AlignPref::Right,
            ..Default::default()
        };
        assert_eq!(
            cell_style(ColumnKind::Integer, &prefs).align,
            TextAlign::Left,
            "数字默认右对齐被显式覆盖为左"
        );
        assert_eq!(
            cell_style(ColumnKind::Text, &prefs).align,
            TextAlign::Center
        );
        assert_eq!(
            cell_style(ColumnKind::Boolean, &prefs).align,
            TextAlign::Right
        );
        // 时间 / BLOB 没覆盖：继承类型默认（左）
        assert_eq!(
            cell_style(ColumnKind::Timestamp, &prefs).align,
            TextAlign::Left
        );
        assert_eq!(
            cell_style(ColumnKind::Binary, &prefs).align,
            TextAlign::Left
        );
        // 数字被覆盖为左后，等宽仍然不引（整数不是等宽族）
        assert!(!cell_style(ColumnKind::Integer, &prefs).monospace);
    }

    /// 【类型样式】前景色角色进 CellStyle（渲染时映射主题色；默认不染色）。
    #[test]
    fn the_cell_style_carries_the_color_role() {
        let prefs = TypeStylePrefs {
            color_numeric: ColorRolePref::Info,
            color_blob: ColorRolePref::Warning,
            ..Default::default()
        };
        assert_eq!(
            cell_style(ColumnKind::Integer, &prefs).color_role,
            ColorRolePref::Info
        );
        assert_eq!(
            cell_style(ColumnKind::Binary, &prefs).color_role,
            ColorRolePref::Warning
        );
        assert_eq!(
            cell_style(ColumnKind::Text, &prefs).color_role,
            ColorRolePref::Inherit
        );
    }

    /// 【类型样式】显示文本纯函数：NULL 占位 / 布尔映射 / 数字格式；原文路径不动。
    #[test]
    fn format_cell_text_applies_type_style_only_to_display() {
        let prefs = TypeStylePrefs {
            null_placeholder: workbench_shell::result_style::NullPlaceholderPref::Dash,
            bool_display: workbench_shell::result_style::BoolDisplayPref::Cn,
            number_format: workbench_shell::result_style::NumberFormatPref::Grouped,
            ..Default::default()
        };
        // NULL 占位
        assert_eq!(format_cell_text(ColumnKind::Text, "NULL", &prefs), "—");
        // 布尔映射（原文判定）
        assert_eq!(format_cell_text(ColumnKind::Boolean, "true", &prefs), "是");
        assert_eq!(format_cell_text(ColumnKind::Boolean, "false", &prefs), "否");
        assert_eq!(format_cell_text(ColumnKind::Boolean, "1", &prefs), "1", "未知形态保持原文");
        // 数字千分位（整数值分组）
        assert_eq!(format_cell_text(ColumnKind::Integer, "1234567", &prefs), "1,234,567");
        // 非数字 / 不可解析保持原文
        assert_eq!(format_cell_text(ColumnKind::Text, "1234567", &prefs), "1234567");
        assert_eq!(format_cell_text(ColumnKind::Integer, "abc", &prefs), "abc");
    }

    /// 【类型样式】固定两位小数 / 科学计数只改显示。
    #[test]
    fn format_cell_text_number_modes() {
        let fixed = TypeStylePrefs {
            number_format: workbench_shell::result_style::NumberFormatPref::Fixed2,
            ..Default::default()
        };
        let sci = TypeStylePrefs {
            number_format: workbench_shell::result_style::NumberFormatPref::Scientific,
            ..Default::default()
        };
        assert_eq!(format_cell_text(ColumnKind::Decimal, "1234.5", &fixed), "1234.50");
        assert_eq!(format_cell_text(ColumnKind::Float, "123456.789", &sci), "1.23456789e5");
        // 解析失败回退原文
        assert_eq!(format_cell_text(ColumnKind::Decimal, "12.3.x", &fixed), "12.3.x");
        // 原始值模式一个字符不改
        assert_eq!(
            format_cell_text(ColumnKind::Integer, "00123", &TypeStylePrefs::default()),
            "00123"
        );
    }

    /// 表头类型小标签：驱动报了才摆（行号槽永不摆），名字**原样**（方言名照显示）
    ///
    /// 同一条用例兼验“换了一份结果/清空之后类型不跟过来”：漏给就是“没类型”，
    /// 而**没类型这一档就是今天**（表头只有一个列名）。
    #[test]
    fn the_header_shows_the_type_only_when_the_driver_reported_one() {
        let mut grid = grid();
        assert!(grid.type_label(0).is_none(), "行号槽不是数据");
        assert!(grid.type_label(1).is_none(), "没带类型 = 不摆标签");
        assert!(grid.type_label(2).is_none());
        assert_eq!(grid.column_kind(1), ColumnKind::Unknown);

        grid.set_column_types(vec![
            Some(ColumnType::parse("bigint")),
            Some(ColumnType::parse("numeric(38,10)")),
        ]);
        assert_eq!(grid.type_label(1), Some("bigint"));
        assert_eq!(
            grid.type_label(2),
            Some("numeric(38,10)"),
            "表头显示的是驱动报的**原样名字**（带参数也照显）"
        );
        assert_eq!(grid.type_label(0), None, "行号槽没有类型可谈");
        assert_eq!(grid.type_label(9), None, "越界不 panic");
        assert_eq!(grid.column_kind(1), ColumnKind::Integer);
        assert_eq!(grid.column_kind(2), ColumnKind::Decimal);
        assert_eq!(
            grid.column_kind(0),
            ColumnKind::Unknown,
            "行号槽永远是无类型档"
        );

        // 换一份结果：上一份的类型不许跟过来（面板漏给 = 没类型，与今天一致）
        grid.set_data(vec!["a".to_string()], vec![vec!["1".to_string()]]);
        assert_eq!(grid.type_label(1), None);
        assert_eq!(grid.column_kind(1), ColumnKind::Unknown);

        // 一条列报了、另一条没报：没报的那条不摆标签
        grid.set_column_types(vec![Some(ColumnType::parse("uuid")), None]);
        assert_eq!(grid.type_label(1), Some("uuid"));
        assert_eq!(grid.type_label(2), None);

        grid.clear("执行失败：boom");
        assert_eq!(grid.column_kind(1), ColumnKind::Unknown);
        assert!(grid.type_label(1).is_none());
    }

    /// 没有筛选也没有排序时**不物化**视图行序（入库 / 切结果集时省掉的正是这份整表映射）；
    /// 而“不物化”必须与“物化一份恒等映射”给出**一模一样**的行——否则就是省错了
    #[test]
    fn the_view_order_is_not_materialized_until_a_filter_or_sort_needs_it() {
        let mut grid = grid_with(&[&["2", "orders"], &["1", "users"]]);
        assert!(
            !grid.view_order_materialized_for_test(),
            "刚入库：恒等，不建映射"
        );
        let plain = grid.visible_rows();
        assert_eq!(plain.len(), 2);

        // 筛一下：这时才建（本来就要逐行看一遍）
        grid.set_filter("users");
        assert!(grid.view_order_materialized_for_test());
        assert_eq!(grid.visible_row_count(), 1);
        assert_eq!(grid.cell(0, 2), "users");

        // 清掉筛选：又回到恒等（不是“留一份恰好是全集的映射”）
        grid.set_filter("");
        assert!(!grid.view_order_materialized_for_test());
        assert_eq!(grid.visible_rows(), plain, "与物化版给出的行必须一模一样");

        // 排序也是（用户真点了列头才建）
        grid.apply_sort(Some((0, false)));
        assert!(grid.view_order_materialized_for_test());
        assert_eq!(grid.visible_rows()[0][0], "1");
        grid.apply_sort(None);
        assert!(!grid.view_order_materialized_for_test());
        assert_eq!(
            grid.visible_rows(),
            plain,
            "取消排序回原顺序（不是部分退还）"
        );

        grid.clear("执行失败：boom");
        assert!(!grid.view_order_materialized_for_test());
    }

    /// 筛选匹配的口径（表驱动）：与 `to_lowercase().contains` 一致；ASCII 边上零分配
    ///
    /// `needle` 的契约是**已小写**（调用方 `to_lowercase` 一次，不给每格都分一次）；
    /// ASCII 快路径两边大小写都不敏感，非 ASCII 回落路径按小写词比。
    #[test]
    fn the_filter_matcher_keeps_the_lowercase_semantics() {
        let cases: &[(&str, &str, bool)] = &[
            ("orders", "orders", true),
            ("ORDERS", "orders", true),
            ("Orders_Archive", "orders", true),
            ("users", "orders", false),
            ("", "orders", false),
            ("ord", "orders", false),
            ("1", "orders", false),
            // 非 ASCII：回落 `to_lowercase`（大小写仍然不敏感，中文不受影响）
            ("Ärger", "ärger", true),
            ("ärger", "ärger", true),
            ("订单_2026", "订单", true),
            ("订单_2026", "2026", true),
            ("订单", "orders", false),
        ];
        for (cell, needle, expected) in cases {
            assert_eq!(
                cell_matches(cell, needle),
                *expected,
                "格 {cell:?} 找 {needle:?}"
            );
        }

        // 格内区间：两边都是 ASCII 才给得出；含非 ASCII 时是 `None`（命中但无区间）
        assert_eq!(find_in_cell("Orders_Archive", "orders"), Some(Some(0..6)));
        assert_eq!(find_in_cell("订单_2026", "2026"), Some(None));
        assert_eq!(find_in_cell("plain", ""), None, "空词不命中任何格");
        assert_eq!(find_in_cell("plain", "plainly"), None, "词比格长");
    }

    /// 结果内查找：只看**数据列**（`#` 槽不算）、按视图行序、找完一圈回到开头
    #[test]
    fn finding_in_the_result_walks_the_visible_rows_and_wraps() {
        let mut grid = grid_with(&[&["1", "orders"], &["2", "users"], &["3", "orders_archive"]]);
        assert_eq!(
            grid.find_next("orders", None),
            Some(ResultMatch {
                row: 0,
                col: 2,
                range: Some(0..6),
            }),
            "第一处命中的是**数据列**（行号槽不是数据）"
        );
        assert_eq!(
            grid.find_next("orders", Some((0, 2))).map(|hit| hit.row),
            Some(2),
            "从上一处继续：中间那行的第二列（users）不命中，继续往后找"
        );
        assert_eq!(
            grid.find_next("orders", Some((2, 2))).map(|hit| hit.row),
            Some(0),
            "找不到就绕回开头（表是圆的）"
        );

        // 行号槽里的数字不算命中（`#` 显示的是视图行号，不是数据）
        assert_eq!(
            grid.find_next("3", None),
            Some(ResultMatch {
                row: 2,
                col: 1,
                range: Some(0..1),
            })
        );
        assert_eq!(grid.find_next("nope", None), None);
        assert_eq!(grid.find_next("   ", None), None, "空词不找任何东西");

        // 含非 ASCII 的格：命中但不给格内区间（宿主退化成整格高亮）
        let chinese = grid_with(&[&["1", "订单_2026"]]);
        assert_eq!(
            chinese.find_next("2026", None),
            Some(ResultMatch {
                row: 0,
                col: 2,
                range: None,
            })
        );

        // 筛选之后只找**看得见的行**（找到看不见的行等于没找到）
        grid.set_filter("users");
        assert_eq!(grid.find_next("orders", None), None);
        assert_eq!(grid.find_next("users", None).map(|hit| hit.row), Some(0));
    }

    /// 【B15】换一份结果：排序跟不过来（列都可能不是同一批），筛选词留着（用户还在找同样的东西）
    #[test]
    fn switching_data_clears_sort_but_keeps_the_filter() {
        let mut grid = grid_with(&[&["2", "orders"], &["1", "users"]]);
        grid.set_filter("orders");
        grid.apply_sort(Some((0, true)));
        assert_eq!(grid.sort_state(), Some((0, true)));

        grid.set_data(
            vec!["id".to_string(), "name".to_string()],
            vec![vec!["5".to_string(), "orders".to_string()]],
        );
        assert_eq!(grid.sort_state(), None, "换结果不该继承旧的排序");
        assert_eq!(grid.filter(), "orders", "筛选词留着");
        assert_eq!(grid.visible_row_count(), 1);
    }

    #[test]
    fn columns_and_rows_follow_the_data() {
        let grid = grid();
        let columns = grid.columns_for_test();
        assert_eq!(columns, ["id".to_string(), "name".to_string()]);
        assert_eq!(grid.row_count_for_test(), 2);
    }

    #[test]
    fn empty_grid_says_why_instead_of_showing_a_blank_panel() {
        let grid = ResultGridDelegate::empty("尚未执行");
        assert_eq!(grid.row_count_for_test(), 0);
        assert!(grid.columns_for_test().is_empty());
    }

    #[test]
    fn clearing_drops_previous_rows() {
        let mut grid = grid();
        grid.clear("执行失败：boom");
        assert_eq!(grid.row_count_for_test(), 0);
        assert!(grid.columns_for_test().is_empty());
    }

    /// 【B15】列头可排序：数据列给 `Some(Default)`（可排但未排），行号槽不给
    ///
    /// 组件库对 `sort: None` 的列**不响应点击**（`perform_sort` 里直接 return），
    /// 所以这条钉的是“列头真的能点下去”。
    #[gpui_kit::test]
    fn data_columns_are_sortable_from_the_header(cx: &mut gpui_kit::TestAppContext) {
        cx.update(gpui_kit::init);
        let mut grid = grid();
        cx.update(|cx: &mut App| {
            assert_eq!(grid.column(0, cx).sort, None, "行号槽不是数据列");
            assert_eq!(grid.column(1, cx).sort, Some(ColumnSort::Default));
            assert_eq!(grid.column(2, cx).sort, Some(ColumnSort::Default));
        });
        // 排上以后指示器要回得去（组件库 refresh 后会重新问 column）
        grid.apply_sort(Some((0, false)));
        cx.update(|cx: &mut App| {
            assert_eq!(grid.column(1, cx).sort, Some(ColumnSort::Ascending));
            assert_eq!(grid.column(2, cx).sort, Some(ColumnSort::Default));
        });
    }

    /// 【P1-5】拖过的列宽按列名记进 delegate，重建表头（`column()` 重取）时恢复；
    /// 行号槽宽度钉死不跟事件；换结果集后旧宽度清空回默认档。
    #[gpui_kit::test]
    fn dragged_column_widths_are_remembered_by_column_name(cx: &mut gpui_kit::TestAppContext) {
        cx.update(gpui_kit::init);
        let mut grid = grid();
        // 没拖过：数据列走默认档，行号槽钉死
        cx.update(|cx: &mut App| {
            assert_eq!(grid.column(1, cx).width, crate::ui::RESULT_COLUMN_WIDTH);
            assert_eq!(grid.column(2, cx).width, crate::ui::RESULT_COLUMN_WIDTH);
            assert_eq!(grid.column(0, cx).width, crate::ui::RESULT_ROW_NUMBER_WIDTH);
        });

        // 组件拖完 mouse-up 报全部列宽：下标 0 = 行号槽，1 = id，2 = name
        grid.set_column_widths(&[
            crate::ui::RESULT_ROW_NUMBER_WIDTH,
            gpui_kit::px(240.),
            gpui_kit::px(96.),
        ]);
        cx.update(|cx: &mut App| {
            assert_eq!(grid.column(1, cx).width, gpui_kit::px(240.), "id 列回填用户宽度");
            assert_eq!(grid.column(2, cx).width, gpui_kit::px(96.), "name 列回填用户宽度");
            assert_eq!(
                grid.column(0, cx).width,
                crate::ui::RESULT_ROW_NUMBER_WIDTH,
                "行号槽宽度钉死，不跟事件跑"
            );
        });

        // 换一份结果集：列集合变了，旧宽度清空，回到默认档
        grid.set_data(
            vec!["id".to_string(), "name".to_string()],
            vec![vec!["1".to_string(), "a".to_string()]],
        );
        cx.update(|cx: &mut App| {
            assert_eq!(grid.column(1, cx).width, crate::ui::RESULT_COLUMN_WIDTH);
            assert_eq!(grid.column(2, cx).width, crate::ui::RESULT_COLUMN_WIDTH);
        });
    }

    /// 【P1-5】列宽事件下标是视图列序：隐藏列后要经 `data_column` 换算，
    /// 宽度才落到对得上名的数据列上（否则隐藏一列后续列宽度整体错位）。
    #[gpui_kit::test]
    fn column_widths_map_around_hidden_columns(cx: &mut gpui_kit::TestAppContext) {
        cx.update(gpui_kit::init);
        let mut grid = grid();
        // 隐藏数据列 0（id）：视图只剩 name（数据列 1），视图列 1 = name
        grid.hide_column(0);
        grid.set_column_widths(&[crate::ui::RESULT_ROW_NUMBER_WIDTH, gpui_kit::px(200.)]);
        cx.update(|cx: &mut App| {
            assert_eq!(
                grid.column(1, cx).width,
                gpui_kit::px(200.),
                "隐藏 id 后，视图第一列 name 拿到对应的用户宽度"
            );
        });
    }

    /// 列数与行数要和真实数据一致（delegate 是网格唯一的取数口）
    ///
    /// **列数比数据列多 1**：首列是行号槽（原型 §2.4）。
    #[gpui_kit::test]
    fn delegate_reports_counts(cx: &mut gpui_kit::TestAppContext) {
        cx.update(gpui_kit::init);
        let grid = grid();
        cx.update(|cx: &mut App| {
            assert_eq!(grid.columns_count(cx), 3, "行号槽 + 两列数据");
            assert_eq!(grid.rows_count(cx), 2);
        });
    }

    /// 首列是行号槽：1 基、与数据错开一位、不进导出文本的数据
    #[gpui_kit::test]
    fn the_first_column_is_a_row_number_gutter(cx: &mut gpui_kit::TestAppContext) {
        cx.update(gpui_kit::init);
        let grid = grid();
        cx.update(|cx: &mut App| {
            assert_eq!(grid.cell_text(0, 0, cx), "1", "行号是 1 基");
            assert_eq!(grid.cell_text(1, 0, cx), "2");
            assert_eq!(grid.cell_text(0, 1, cx), "1", "数据从第 2 列开始");
            assert_eq!(grid.cell_text(1, 2, cx), "NULL");
        });
    }

    /// 空网格不画行号槽（没数据时那一列是空的）
    #[gpui_kit::test]
    fn an_empty_grid_has_no_gutter(cx: &mut gpui_kit::TestAppContext) {
        cx.update(gpui_kit::init);
        let grid = ResultGridDelegate::empty("尚未执行");
        cx.update(|cx: &mut App| assert_eq!(grid.columns_count(cx), 0));
    }

    /// 截断提示按原型 §2.4 的口径（`已截断至 N 行`），数字带千分位
    #[test]
    fn truncation_hint_names_the_real_row_count() {
        assert_eq!(truncated_hint(10_000), "已截断至 10,000 行");
    }

    /// 工具栏左段：`行数 N` │ `耗时 1.2s` │ `来源摘要` │ `连接名`，没有的东西不占位
    #[test]
    fn toolbar_segments_follow_the_prototype_order() {
        let full = ResultToolbar {
            rows: Some(1_204),
            affected_rows: None,
            failed: false,
            elapsed_ms: Some(1_200),
            // 【B15】血缘排在连接之前（“这份是怎么来的”比“从哪条连接来”更常被问）
            lineage: Some("下发筛选".to_string()),
            connection: Some("●P·orders".to_string()),
            ..Default::default()
        };
        assert_eq!(
            full.segments(),
            ["行数 1,204", "耗时 1.2s", "下发筛选", "●P·orders"]
        );

        // 写语句：报影响行数，没有行数段
        let write = ResultToolbar {
            rows: None,
            affected_rows: Some(3),
            failed: false,
            elapsed_ms: Some(35),
            connection: None,
            ..Default::default()
        };
        assert_eq!(write.segments(), ["影响 3 行", "耗时 35 ms"]);

        // 失败：只说“失败”（原因在错误卡片里）
        let failed = ResultToolbar {
            failed: true,
            ..Default::default()
        };
        assert_eq!(failed.segments(), ["失败"]);

        // 什么都没执行过：一段都不摆
        assert!(ResultToolbar::default().segments().is_empty());
    }

    /// 状态行：`共 N 行` │ `已选第 M 行`，没选中就直说
    #[test]
    fn status_segments_report_total_and_selection() {        let idle = ResultStatus {
            total_rows: 1_204,
            selected_row: None,
            selected_cell: None,
            truncated_hint: None,
            has_more: false,
        };
        assert_eq!(status_segments(&idle), ("共 1,204 行".to_string(), "未选中行".to_string()));

        let picked = ResultStatus {
            selected_row: Some(12),
            ..idle.clone()
        };
        assert_eq!(status_segments(&picked).1, "已选第 12 行");
    }

    /// 【B5b】还有下一段时总数是 `1,000+`（“至少这么多”，不是“共”）
    #[test]
    fn a_partial_result_shows_a_plus_after_the_count() {
        let partial = ResultStatus {
            total_rows: 1_000,
            selected_row: None,
            selected_cell: None,
            truncated_hint: None,
            has_more: true,
        };
        assert_eq!(status_segments(&partial).0, "共 1,000+ 行");
    }

    /// 【P0-2a】状态行分段语义：分段中提示「还有更多」；撞单段上限且到底才出「不完整」警告；
    /// 正常取完两者都不提示；窗口代价 tooltip 只在分段中 / 截断时挂。
    #[test]
    fn status_fetch_and_truncation_hints_follow_state_combination() {
        // 正常取完：无任何额外提示
        let done = ResultStatus {
            total_rows: 120,
            selected_row: None,
            selected_cell: None,
            truncated_hint: None,
            has_more: false,
        };
        assert_eq!(fetch_more_hint(&done), None);
        assert_eq!(truncation_notice(&done), None);
        assert!(!window_fetch_active(&done));

        // 分段中：「还有更多」，不出截断警告
        let partial = ResultStatus { has_more: true, ..done.clone() };
        assert_eq!(fetch_more_hint(&partial), Some("还有更多 · 滚动到底继续加载"));
        assert_eq!(truncation_notice(&partial), None);
        assert!(window_fetch_active(&partial));

        // 撞上限、已到底：出「不完整」警告，文案点明 10,000 上限口径
        let capped = ResultStatus {
            truncated_hint: Some("已截断至 10,000 行".to_string()),
            has_more: false,
            ..done.clone()
        };
        assert_eq!(fetch_more_hint(&capped), None);
        let notice = truncation_notice(&capped).expect("到底且截断必出警告");
        assert!(notice.contains("10,000"), "文案须点明单段上限口径");
        assert!(notice.contains("不完整"));
        assert!(window_fetch_active(&capped));

        // 防御组合（分段中 + 截断信号）：截断警告让位给「还有更多」，不重复出警告
        let mixed = ResultStatus {
            truncated_hint: Some("已截断至 10,000 行".to_string()),
            has_more: true,
            ..done.clone()
        };
        assert!(truncation_notice(&mixed).is_none());
        assert!(window_fetch_active(&mixed));

        // 窗口代价文案与 engine 分段取舍一致；上限口径对齐 engine MAX_QUERY_ROWS
        assert!(WINDOW_FETCH_NOTICE.contains("ORDER BY"));
        assert_eq!(MAX_SINGLE_SEGMENT_ROWS, 10_000);
    }

    /// 【#26】底部动作状态行占位钮清单：顺序对齐 DBeaver 三段（写操作 ‖ 翻页 ‖ 系统），
    /// 「下页」是现有取下一段真钮，不进占位清单；本轮全部 disabled，接线归下一轮行级模型
    #[test]
    fn status_action_placeholders_follow_dbeaver_group_order() {
        let slots = status_action_placeholders();
        assert_eq!(slots.len(), 12, "占位钮共 12 个（下页真钮另由 controls.more 注入）");

        // 写操作段：保存 · 取消 · 编辑 · 新增行 · 复制行 · 删除行 · 撤销 · 重做
        let write: Vec<IconName> = slots[0..FIRST_PAGER_ACTION]
            .iter()
            .map(|(icon, _)| *icon)
            .collect();
        assert_eq!(
            write,
            vec![
                IconName::Save,
                IconName::X,
                IconName::Pencil,
                IconName::Plus,
                IconName::CopyPlus,
                IconName::Trash,
                IconName::Undo2,
                IconName::Redo2,
            ]
        );

        // 翻页段：首页 · 上页 · 末页（下页 = 取下一段真钮，占位清单里没有 ChevronRight）
        let pager: Vec<IconName> = slots[FIRST_PAGER_ACTION..FIRST_SYSTEM_ACTION]
            .iter()
            .map(|(icon, _)| *icon)
            .collect();
        assert_eq!(pager, vec![IconName::SkipBack, IconName::ChevronLeft, IconName::SkipForward]);
        assert!(!slots.iter().any(|(icon, _)| *icon == IconName::ChevronRight));

        // 系统段：导出（设置按钮已移除，DBeaver 状态栏没有设置按钮）
        let system: Vec<IconName> = slots[FIRST_SYSTEM_ACTION..]
            .iter()
            .map(|(icon, _)| *icon)
            .collect();
        assert_eq!(system, vec![IconName::Download]);

        // 每个占位钮都带可读 tooltip（说明为何禁用 / 待哪一轮接线）
        assert!(slots.iter().all(|(_, tooltip)| !tooltip.is_empty()));
    }

    /// 列头菜单（DBeaver 箭头）：列级子集——排序升/降、复制此列、冻结、隐藏、
    /// 统计；值与行动作（预览 / 按值筛选 / 复制整行）不属于列头；没隐藏列就不摆
    /// 「显示全部列」
    #[test]
    fn column_menu_is_the_column_level_subset() {
        let target = ContextTarget {
            value: "x".to_string(),
            column_name: "id".to_string(),
            column: 1,
            row: 3,
            rows: 12,
            frozen: false,
            hidden_columns: 0,
            insight: false,
            table: None,
            column_filter_active: false,
            column_values: Vec::new(),
            sort_active: false,
            row_ref: RowRef::default(),
            editable: true,
            is_deleted: false,
        };
        let items = column_menu_items(&target);
        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        assert!(labels.contains(&"按「id」升序（下发源库）"));
        assert!(labels.contains(&"按「id」降序（下发源库）"));
        assert!(labels.contains(&"复制此列（12 个取值）"));
        assert!(labels.contains(&"冻结「id」"));
        assert!(labels.contains(&"隐藏「id」"));
        assert!(labels.contains(&"列统计「id」"));
        // 值与行动作不属于列头
        assert!(!labels.iter().any(|l| l.contains("查看完整值")));
        assert!(!labels.iter().any(|l| l.contains("按值筛选")));
        assert!(!labels.iter().any(|l| l.contains("复制整行")));
        // 没隐藏列就不摆「显示全部列」
        assert!(!labels.contains(&"显示全部列"));
    }

    /// 列浏览器清单菜单 = 列头菜单的本地动作子集：复制 / 冻结 / 隐藏 / 列统计；
    /// 排序下发 / 值筛选 / 洞察 / 查看完整值 / 复制整行不进清单入口（前者依赖宿主
    /// 钩子，后者属于格 / 行交互，见拍板口径）。
    #[test]
    fn column_browser_menu_is_local_column_subset() {
        let target = target("x", "id", 1);
        let items = column_browser_menu_items(&target);
        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        assert!(labels.contains(&"复制此列（2 个取值）"));
        assert!(labels.contains(&"冻结「id」"));
        assert!(labels.contains(&"隐藏「id」"));
        assert!(labels.contains(&"列统计「id」"));
        // 不摆排序下发 / 值筛选 / 洞察 / 值与行动作
        assert!(!labels.iter().any(|l| l.contains("升序") || l.contains("降序")));
        assert!(!labels.iter().any(|l| l.contains("按值筛选")));
        assert!(!labels.iter().any(|l| l.contains("洞察")));
        assert!(!labels.iter().any(|l| l.contains("查看完整值")));
        assert!(!labels.iter().any(|l| l.contains("复制整行")));
        // 无隐藏列 / 未排序 → 不摆条件项
        assert!(!labels.contains(&"显示全部列"));
        assert!(!labels.iter().any(|l| l.contains("取消排序")));
    }

    /// 清单菜单条件项：排序生效 → 取消排序；有隐藏列 → 显示全部列；冻结中 → 文案反向。
    #[test]
    fn column_browser_menu_shows_conditional_items() {
        let mut target = target("", "name", 2);
        target.frozen = true;
        target.hidden_columns = 2;
        target.sort_active = true;
        let items = column_browser_menu_items(&target);
        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        assert!(labels.contains(&"取消冻结此列"));
        assert!(labels.contains(&"显示全部列"));
        assert!(labels.contains(&"取消排序「name」"));
    }

    /// 清单菜单动作归属：本地动作直接映射到对应 `ContextAction`，列号不串位。
    #[test]
    fn column_browser_menu_actions_map_to_local_column_actions() {
        let target = target("", "name", 3);
        let items = column_browser_menu_items(&target);
        let by_label: std::collections::HashMap<&str, &ContextAction> =
            items.iter().map(|i| (i.label.as_str(), &i.action)).collect();
        assert!(matches!(
            by_label["复制此列（2 个取值）"],
            ContextAction::CopyColumn(3)
        ));
        assert!(matches!(
            by_label["冻结「name」"],
            ContextAction::ToggleFreeze(3)
        ));
        assert!(matches!(
            by_label["隐藏「name」"],
            ContextAction::HideColumn(3)
        ));
        assert!(matches!(
            by_label["列统计「name」"],
            ContextAction::ColumnStats { column: 3 }
        ));
    }

    /// 列头菜单的条件项：隐藏了列 → 摆「显示全部列」；可洞察 → 摆「洞察此列」；
    /// 冻结中 → 文案反向（取消冻结）
    #[test]
    fn column_menu_shows_conditional_items() {
        let target = ContextTarget {
            value: String::new(),
            column_name: "name".to_string(),
            column: 2,
            row: 0,
            rows: 5,
            frozen: true,
            hidden_columns: 2,
            insight: true,
            table: Some("t_user".to_string()),
            column_filter_active: false,
            column_values: Vec::new(),
            sort_active: false,
            row_ref: RowRef::default(),
            editable: true,
            is_deleted: false,
        };
        let items = column_menu_items(&target);
        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        assert!(labels.contains(&"显示全部列"));
        assert!(labels.contains(&"洞察「name」这一列"));
        assert!(labels.contains(&"取消冻结此列"));
        // 表名只喂给 INSERT（右键项）；列头菜单不含它
        assert!(!labels.iter().any(|l| l.contains("INSERT")));
    }

    /// 列头菜单筛选段：值列表（去重取值，按出现顺序）平铺成「按值筛选「值」」项，
    /// 第一项带分隔线；筛选生效时才摆「清除此列筛选」
    #[test]
    fn column_menu_lists_distinct_values_and_clear_only_when_active() {
        let target = ContextTarget {
            value: String::new(),
            column_name: "status".to_string(),
            column: 1,
            row: 0,
            rows: 9,
            frozen: false,
            hidden_columns: 0,
            insight: false,
            table: None,
            column_filter_active: false,
            column_values: vec!["paid".to_string(), "NULL".to_string(), "refunded".to_string()],
            sort_active: false,
            row_ref: RowRef::default(),
            editable: true,
            is_deleted: false,
        };
        let items = column_menu_items(&target);
        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        assert!(labels.contains(&"按值筛选「paid」"));
        assert!(labels.contains(&"按值筛选「NULL」"));
        assert!(labels.contains(&"按值筛选「refunded」"));
        // 值列表第一项带分隔线（与上方排序段隔开）
        let first_value = items
            .iter()
            .find(|i| i.label.contains("按值筛选"))
            .unwrap();
        assert!(first_value.separator_before);
        // 没筛不摆清除项
        assert!(!labels.contains(&"清除此列筛选"));

        // 筛选生效：摆「清除此列筛选」（分隔线隔开），值列表照旧
        let active = ContextTarget {
            column_filter_active: true,
            ..target.clone()
        };
        let items = column_menu_items(&active);
        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        assert!(labels.contains(&"清除此列筛选"));
        let clear = items.iter().find(|i| i.label == "清除此列筛选").unwrap();
        assert!(clear.separator_before);
        // 空值列表 = 不摆筛选段（没数据这一列自然没有可选值）
        let empty = ContextTarget {
            column_values: Vec::new(),
            ..target
        };
        assert!(!column_menu_items(&empty)
            .iter()
            .any(|i| i.label.contains("按值筛选")));
    }

    /// 【筛选】去重取值：按出现顺序去重、到 `limit` 就停；列越界 / 无行 = 空
    #[test]
    fn column_distinct_values_dedupes_in_order_and_limits() {
        let rows: Vec<Vec<String>> = vec![
            vec!["a".into(), "x".into()],
            vec!["b".into(), "x".into()],
            vec!["c".into(), "y".into()],
            vec!["d".into(), "z".into()],
            vec!["e".into(), "z".into()],
        ];
        // 第 1 列：x, y, z（x 去重）
        let got = column_distinct_values(&rows, 1, 8);
        assert_eq!(got, ["x", "y", "z"].map(String::from));
        // limit 生效：最多 2 个
        let got = column_distinct_values(&rows, 1, 2);
        assert_eq!(got, ["x", "y"].map(String::from));
        // 列越界 / 无行 = 空
        assert!(column_distinct_values(&rows, 9, 8).is_empty());
        assert!(column_distinct_values(&[], 0, 8).is_empty());
        // NULL 是字面量，原样计入（值列表里能筛 NULL）
        let rows = vec![vec!["NULL".to_string()], vec!["NULL".to_string()], vec!["ok".to_string()]];
        let got = column_distinct_values(&rows, 0, 8);
        assert_eq!(got, ["NULL", "ok"].map(String::from));
    }

    /// 【筛选】列级筛选只命中目标列（包含匹配），且与全局筛选词叠加取交集
    #[test]
    fn column_filter_scopes_to_column_and_stacks_with_global() {
        let mut grid = grid_with(&[
            &["1", "alice"],
            &["2", "bob"],
            &["3", "alice"],
            &["4", "carol"],
        ]);
        // 列筛选 = 第 1 列（name）含 "ali" → 行 0、2
        grid.set_column_filter(Some((1, "ali".to_string())));
        assert_eq!(
            grid.visible_rows(),
            vec![
                vec!["1".to_string(), "alice".to_string()],
                vec!["3".to_string(), "alice".to_string()],
            ]
        );
        assert!(grid.has_column_filter(1));
        assert!(!grid.has_column_filter(0));
        assert_eq!(grid.column_filter(), Some((1, "ali")));

        // 叠加全局筛选（id 列含 "3"）→ 只剩行 2
        grid.set_filter("3");
        assert_eq!(grid.visible_rows(), vec![vec!["3".to_string(), "alice".to_string()]]);

        // 清除列筛选 → 全局筛选单独生效
        grid.set_column_filter(None);
        assert_eq!(grid.visible_rows(), vec![vec!["3".to_string(), "alice".to_string()]]);
        assert!(!grid.has_column_filter(1));
        assert!(grid.column_filter().is_none());
    }

    /// 【类型感知】按列类型校验编辑值：数字列必须数字、布尔取值受限、UUID / 日期
    /// 查形状；空与 `NULL` 一律放行（清值是“删值”语义）；文本 / 未知列不校验
    #[test]
    fn edit_validation_is_type_aware_and_allows_clear() {
        use crate::store::ColumnKind as K;
        // 整数：合法 / 非法 / 空与 NULL 放行
        assert!(validate_edit_value(K::Integer, "42").is_none());
        assert!(validate_edit_value(K::Integer, "-7").is_none());
        assert!(validate_edit_value(K::Integer, "3.14").is_some());
        assert!(validate_edit_value(K::Integer, "abc").is_some());
        assert!(validate_edit_value(K::Integer, "").is_none());
        assert!(validate_edit_value(K::Integer, "NULL").is_none());
        // 浮点 / 定点
        assert!(validate_edit_value(K::Float, "3.14").is_none());
        assert!(validate_edit_value(K::Decimal, "1e3").is_none());
        assert!(validate_edit_value(K::Float, "x").is_some());
        // 布尔：true/false/1/0/t/f/yes/no（大小写不限）
        for ok in ["true", "false", "1", "0", "t", "f", "yes", "no", "TRUE"] {
            assert!(validate_edit_value(K::Boolean, ok).is_none(), "{ok}");
        }
        assert!(validate_edit_value(K::Boolean, "maybe").is_some());
        // UUID 形状（8-4-4-4-12）
        assert!(validate_edit_value(K::Uuid, "550e8400-e29b-41d4-a716-446655440000").is_none());
        assert!(validate_edit_value(K::Uuid, "550e8400e29b41d4a716446655440000").is_some());
        assert!(validate_edit_value(K::Uuid, "hello").is_some());
        // 日期（YYYY-MM-DD + 月/日范围）
        assert!(validate_edit_value(K::Date, "2026-10-24").is_none());
        assert!(validate_edit_value(K::Date, "2026-13-01").is_some());
        assert!(validate_edit_value(K::Date, "2026/10/24").is_some());
        assert!(validate_edit_value(K::Date, "2026-10-32").is_some());
        // 文本 / 未知不校验
        assert!(validate_edit_value(K::Text, "任意 内容!@#").is_none());
        assert!(validate_edit_value(K::Unknown, "任意").is_none());
    }

    /// 耗时：不到一秒报毫秒（`0.0s` 什么都没说），过一秒按人读的写法
    #[test]
    fn duration_text_is_readable() {
        assert_eq!(duration_text(0), "0 ms");
        assert_eq!(duration_text(999), "999 ms");
        assert_eq!(duration_text(1_200), "1.2s");
        assert_eq!(duration_text(102_000), "1m42s");
    }

    /// 千分位（原型里的 `1,204`）
    #[test]
    fn thousands_separates_digit_groups() {
        assert_eq!(thousands(0), "0");
        assert_eq!(thousands(999), "999");
        assert_eq!(thousands(1_000), "1,000");
        assert_eq!(thousands(1_204), "1,204");
        assert_eq!(thousands(12_345_678), "12,345,678");
    }

    /// 【P2-2】BLOB 摘要：正常 hex → `BLOB · N B`；异常形态退回 `None`（按原文显示）
    #[test]
    fn blob_summary_is_the_binary_badge() {
        assert_eq!(
            blob_summary("48656c6c6f"),
            Some("BLOB · 5 B".to_string()),
            "10 个 hex 位 = 5 字节"
        );
        assert_eq!(blob_summary(""), None, "空串不是二进制");
        assert_eq!(blob_summary("abc"), None, "奇数位不猜");
        assert_eq!(blob_summary("zzzzzzzz"), None, "非 hex 字符退回原文");
        assert_eq!(
            blob_summary("0x48656c6c6f"),
            Some("BLOB · 5 B".to_string()),
            "带 0x 前缀也认得（宽松处理）"
        );
    }

    /// 【P0-1】展示裁剪只裁画面串：按 `char` 边界不裁断中文 / emoji；完整值另走
    /// `cell()` / `row_cell_text`（复制 / 导出 / 悬停 / 筛选 / 统计），BLOB 优先摘要由
    /// `render_td` 的分支结构保证，不在这个纯函数里。
    #[test]
    fn clip_cell_display_respects_char_boundary_and_limit() {
        // 恰好上限：原样返回，不加省略号
        let exact = "a".repeat(CELL_DISPLAY_LIMIT);
        assert_eq!(clip_cell_display(&exact, CELL_DISPLAY_LIMIT), exact);

        // 超 1 个字符：前 300 字符 + …，省略号不占额度
        let over = "a".repeat(CELL_DISPLAY_LIMIT + 1);
        let clipped = clip_cell_display(&over, CELL_DISPLAY_LIMIT);
        assert_eq!(clipped.chars().count(), CELL_DISPLAY_LIMIT + 1);
        let body = clipped.strip_suffix('…').expect("超长必带省略号");
        assert_eq!(body, "a".repeat(CELL_DISPLAY_LIMIT));

        // 中文 + emoji：按 char 计数，边界落在字符上，超界 emoji 整字符裁掉、不出现半个
        let unicode = "中".repeat(CELL_DISPLAY_LIMIT) + "😀";
        let u = clip_cell_display(&unicode, CELL_DISPLAY_LIMIT);
        assert!(u.ends_with('…'));
        assert!(!u.contains('😀'));
        assert_eq!(u.chars().count(), CELL_DISPLAY_LIMIT + 1);

        // 多行值：中间换行保留，只截断尾部补省略号
        let multiline = "第一行\n第二行\n".repeat(40);
        let m = clip_cell_display(&multiline, CELL_DISPLAY_LIMIT);
        assert!(m.contains('\n'), "中间换行不应被破坏");
        assert!(m.ends_with('…'));

        // 短串 / NULL / 空串原样返回
        assert_eq!(clip_cell_display("NULL", CELL_DISPLAY_LIMIT), "NULL");
        assert_eq!(clip_cell_display("", CELL_DISPLAY_LIMIT), "");

        // 通用小 limit：5 字符取前 5 + …
        assert_eq!(clip_cell_display("abcdef", 5), "abcde…");
    }

    /// 【P1-4】格内 JSON 美化：对象 / 数组压缩串展开成缩进多行；非 JSON 与裸标量原样交回 None，
    /// 由上层按普通文本裁剪；解析失败不抛错。
    #[test]
    fn pretty_json_only_formats_parseable_objects_and_arrays() {
        // 对象：两空格缩进 + 换行展开
        let obj = r#"{"a":1,"b":"x"}"#;
        assert_eq!(
            pretty_json_display(obj).as_deref(),
            Some("{\n  \"a\": 1,\n  \"b\": \"x\"\n}"),
            "压缩对象展开成缩进多行"
        );
        // 数组同样美化
        let arr = r#"[1,2,3]"#;
        assert_eq!(
            pretty_json_display(arr).as_deref(),
            Some("[\n  1,\n  2,\n  3\n]"),
            "压缩数组展开成缩进多行"
        );
        // 前后空白包裹仍识别（trim 后解析）
        assert!(pretty_json_display("  {\"k\": null}  ").is_some());

        // `{` 开头但不是合法 JSON：返回 None，不猜成 JSON
        assert!(pretty_json_display("{oops").is_none());
        // 裸标量 / 普通文本 / NULL / 空串：不进美化路径
        assert!(pretty_json_display("123").is_none());
        assert!(pretty_json_display("true").is_none());
        assert!(pretty_json_display("\"plain\"").is_none());
        assert!(pretty_json_display("plain text").is_none());
        assert!(pretty_json_display("NULL").is_none());
        assert!(pretty_json_display("").is_none());
    }

    /// 【dbdelve 借鉴】超长值跳过 JSON 美化 / 类型格式化（解析成本有上界）：
    /// 短值仍美化（可读性不变），超长值零拷贝借原文、由 clip 收尾——大 JSON /
    /// 几何文档每帧渲染成本不再随值长线性涨。
    #[test]
    fn overlong_values_skip_pretty_and_clip_original() {
        use std::borrow::Cow;

        // 短 JSON 仍美化（行为不变）
        assert_eq!(
            cell_display_text(r#"{"a":1}"#, ColumnKind::Json, &TypeStylePrefs::default()).as_ref(),
            "{\n  \"a\": 1\n}"
        );
        // 超长 JSON：不解析，原文零拷贝借出（无 pretty 缩进）
        let big = format!("{{\"a\":\"{}\"}}", "x".repeat(CELL_DISPLAY_LIMIT + 10));
        let shown = cell_display_text(&big, ColumnKind::Json, &TypeStylePrefs::default());
        assert!(matches!(shown, Cow::Borrowed(_)), "超长值零拷贝借原文");
        // 统一裁剪收尾：前 300 字符 + 省略号，省略号不占额度
        let clipped = clip_cell_display(&shown, CELL_DISPLAY_LIMIT);
        assert_eq!(clipped.chars().count(), CELL_DISPLAY_LIMIT + 1);
        assert!(clipped.ends_with('…'));
        // 非 JSON 超长文本同样跳过格式化（原文借出）
        let long_text = "plain ".repeat(CELL_DISPLAY_LIMIT);
        assert!(matches!(
            cell_display_text(&long_text, ColumnKind::Text, &TypeStylePrefs::default()),
            Cow::Borrowed(_)
        ));
    }

    /// 【P2】整数单元格千分位：整数值三位一组加逗号、负数保留负号；小数 / 科学计数 /
    /// 文本 / NULL / 空串返回 None（交回普通展示路径，不强行分组）。
    #[test]
    fn integer_cell_values_get_thousands_separators_others_stay_raw() {
        assert_eq!(format_integer_cell("1234567").as_deref(), Some("1,234,567"));
        assert_eq!(format_integer_cell("1234").as_deref(), Some("1,234"));
        assert_eq!(format_integer_cell("42").as_deref(), Some("42"));
        assert_eq!(format_integer_cell("0").as_deref(), Some("0"));
        // 负数：负号保留，绝对值分组
        assert_eq!(format_integer_cell("-1234").as_deref(), Some("-1,234"));
        assert_eq!(format_integer_cell("-42").as_deref(), Some("-42"));
        // 前后空白先 trim 再解析
        assert_eq!(format_integer_cell("  1000  ").as_deref(), Some("1,000"));

        // 小数 / 科学计数 / 文本 / NULL / 空串：不分组
        assert!(format_integer_cell("3.14").is_none());
        assert!(format_integer_cell("1e3").is_none());
        assert!(format_integer_cell("abc").is_none());
        assert!(format_integer_cell("NULL").is_none());
        assert!(format_integer_cell("").is_none());
    }

    /// 【P2-1】隐藏列：视图列序跳过隐藏列，数据 / 类型 / 冻结仍按数据列走；
    /// 复制整行不再带隐藏列；换结果清空隐藏清单
    #[test]
    fn hidden_columns_are_skipped_in_view_but_kept_in_data() {
        let mut grid = ResultGridDelegate::default();
        grid.set_data(
            vec![
                "id".to_string(),
                "name".to_string(),
                "blob".to_string(),
            ],
            vec![
                vec!["1".to_string(), "alice".to_string(), "aabb".to_string()],
                vec!["2".to_string(), "bob".to_string(), "ccdd".to_string()],
            ],
        );

        // 隐藏第 1 数据列（name）：表格只剩 # + id + blob 三列
        grid.hide_column(1);
        assert_eq!(grid.visible_column_count(), 2, "3 数据列 − 1 隐藏");
        assert_eq!(grid.data_column(0), 0, "可见列 0 = 数据列 0");
        assert_eq!(grid.data_column(1), 2, "可见列 1 跳过隐藏列 → 数据列 2");
        assert_eq!(grid.view_column(0), Some(0));
        assert_eq!(grid.view_column(1), None, "隐藏列没有可见下标");
        assert_eq!(grid.cell(0, 2), "aabb", "取值走数据列（blob 的内容还在）");

        // 复制整行：隐藏列不参与（用户看不到的不该拷走）
        assert_eq!(grid.row_text(0), "1\taabb", "TSV 只含可见列");

        // 换一批数据：隐藏清单作废
        grid.set_data(
            vec!["a".to_string(), "b".to_string()],
            vec![vec!["1".to_string(), "2".to_string()]],
        );
        assert_eq!(grid.visible_column_count(), 2, "新结果回到全部列");
    }

    /// 【侧栏】单列取消隐藏：`show_column` 只放回这一列，其它隐藏列原样保留
    /// （与「显示全部」整批清空的粒度区别开——左栏的眼睛是单列粒度）
    #[test]
    fn show_column_restores_only_that_column() {
        let mut grid = ResultGridDelegate::default();
        grid.set_data(
            vec![
                "a".to_string(),
                "b".to_string(),
                "c".to_string(),
            ],
            vec![vec!["1".to_string(), "2".to_string(), "3".to_string()]],
        );
        grid.hide_column(0);
        grid.hide_column(1);
        assert!(grid.is_hidden_column(0) && grid.is_hidden_column(1));
        grid.show_column(0);
        assert!(!grid.is_hidden_column(0), "第 0 列恢复");
        assert!(grid.is_hidden_column(1), "第 1 列仍隐藏");
        assert_eq!(grid.visible_column_count(), 2);
    }

    /// 【呈现】文本视图：可见列头 + 可见行拼 TSV；隐藏列不参与；行序随筛选
    #[test]
    fn text_presentation_skips_hidden_columns_and_follows_view_rows() {
        let mut grid = ResultGridDelegate::default();
        grid.set_data(
            vec![
                "id".to_string(),
                "name".to_string(),
                "blob".to_string(),
            ],
            vec![
                vec!["1".to_string(), "alice".to_string(), "aabb".to_string()],
                vec!["2".to_string(), "bob".to_string(), "ccdd".to_string()],
            ],
        );
        let text = grid.text_presentation();
        assert_eq!(
            text,
            "id\tname\tblob\n1\talice\taabb\n2\tbob\tccdd",
            "全列 TSV（列头 + 行）"
        );
        grid.hide_column(2);
        assert_eq!(
            grid.text_presentation(),
            "id\tname\n1\talice\n2\tbob",
            "隐藏列不参与文本"
        );
        grid.set_filter("bob");
        assert_eq!(grid.text_presentation(), "id\tname\n2\tbob", "筛选只留命中的行");
    }

    /// 【分组】本地 groupby 计数：保留首次出现顺序，NULL 独立成组
    #[test]
    fn group_by_count_keeps_order_and_null_group() {
        let rows = vec![
            vec!["a".to_string(), "x".to_string()],
            vec!["b".to_string(), "NULL".to_string()],
            vec!["a".to_string(), "x".to_string()],
            vec!["c".to_string(), "y".to_string()],
        ];
        let groups = ResultGridDelegate::group_by_count(&rows, 0);
        assert_eq!(
            groups,
            vec![
                ("a".to_string(), 2),
                ("b".to_string(), 1),
                ("c".to_string(), 1),
            ],
            "首现顺序 + 计数"
        );
        let by_null = ResultGridDelegate::group_by_count(&rows, 1);
        assert_eq!(
            by_null,
            vec![("x".to_string(), 2), ("NULL".to_string(), 1), ("y".to_string(), 1)],
            "NULL 独立成组"
        );
        assert!(ResultGridDelegate::group_by_count(&rows, 9).is_empty(), "越界列空组");
    }

    /// 【P2-3】列统计：NULL 计空值，数值列给 min/max/avg/sum，文本列只给计数
    #[test]
    fn column_stats_count_nulls_and_numeric_bounds() {
        let rows = vec![
            vec!["10".to_string(), "alice".to_string(), "NULL".to_string()],
            vec!["20".to_string(), "bob".to_string(), "x".to_string()],
            vec!["NULL".to_string(), "carol".to_string(), "y".to_string()],
            vec!["30".to_string(), "dave".to_string(), "z".to_string()],
        ];
        let numeric = column_stats(&rows, 0);
        assert_eq!(numeric.count, 3, "10/20/30 三个非空");
        assert_eq!(numeric.nulls, 1, "NULL 单计");
        assert_eq!(numeric.min, Some(10.0));
        assert_eq!(numeric.max, Some(30.0));
        assert_eq!(numeric.avg, Some(20.0));
        assert_eq!(numeric.sum, Some(60.0));

        let text = column_stats(&rows, 1);
        assert_eq!(text.count, 4);
        assert_eq!(text.nulls, 0);
        assert_eq!(text.min, None, "文本列不编数值");

        let mixed = column_stats(&rows, 2);
        assert_eq!(mixed.count, 3, "x/y/z 三个非空");
        assert_eq!(mixed.nulls, 1, "NULL 计入空值");
        assert_eq!(mixed.min, None, "x/y/z 解析不了数字");
    }

    /// 【P2-2】列级纯函数与矩阵版同口径：抽列后统计 / 分组结果逐项相等。
    #[test]
    fn column_level_stats_and_grouping_match_matrix_versions() {
        let rows = vec![
            vec!["10".to_string(), "a".to_string()],
            vec!["20".to_string(), "NULL".to_string()],
            vec!["NULL".to_string(), "a".to_string()],
            vec!["30".to_string(), "b".to_string()],
        ];
        let values: Vec<String> = rows.iter().map(|row| row[0].clone()).collect();
        assert_eq!(column_stats_from_values(&values), column_stats(&rows, 0), "统计列级内核与矩阵版一致");

        let name_values: Vec<String> = rows.iter().map(|row| row[1].clone()).collect();
        assert_eq!(group_by_values(&name_values), ResultGridDelegate::group_by_count(&rows, 1), "分组列级内核与矩阵版一致");
    }

    /// 【P2-2】delegate 列级访问器跟随筛选 / 排序后的**视图行序**，结果与矩阵
    /// `visible_rows()` 现算一致——消除整行物化后口径不能变。
    #[test]
    fn delegate_column_accessors_follow_view_rows_like_matrix_version() {
        let mut grid = grid_with(&[&["10", "a"], &["20", "b"], &["NULL", "a"], &["30", "b"]]);

        // 全量视图：统计 / 分组与矩阵版逐项相等
        let rows = grid.visible_rows();
        assert_eq!(grid.column_stats_for(0), column_stats(&rows, 0));
        assert_eq!(grid.group_by_for(1), ResultGridDelegate::group_by_count(&rows, 1));
        assert_eq!(
            grid.group_by_for(1),
            vec![("a".to_string(), 2), ("b".to_string(), 2)],
            "首现顺序 + 计数"
        );

        // 筛选后只剩 name 含 a 的两行：访问器同样只统计视图行
        grid.set_filter("a");
        let filtered = grid.visible_rows();
        assert_eq!(grid.column_stats_for(0), column_stats(&filtered, 0), "统计跟随筛选行序");
        assert_eq!(grid.group_by_for(1), ResultGridDelegate::group_by_count(&filtered, 1), "分组跟随筛选行序");
        assert_eq!(grid.column_stats_for(0).count, 1, "10 那行非空");
        assert_eq!(grid.column_stats_for(0).nulls, 1, "NULL 那行被筛进来");
    }

    /// 【P2-3】展示行：数值给全，纯文本列只给计数两行
    #[test]
    fn stats_lines_are_key_value_pairs() {
        let stats = column_stats(
            &[vec!["1.5".to_string(), "NULL".to_string()]],
            0,
        );
        assert_eq!(
            stats_lines(&stats),
            ["值 1 个", "NULL 0 个", "最小 1.5", "最大 1.5", "均值 1.5", "合计 1.5"]
        );
        assert_eq!(fmt_stat_num(1.0), "1", "去尾零");
        assert_eq!(fmt_stat_num(1.500000), "1.5");
    }

    /// 【P2-1】隐藏到只剩 1 列就拒绝（全藏了表格没有内容）
    #[test]
    fn hiding_stops_before_the_last_column() {
        let mut grid = ResultGridDelegate::default();
        grid.set_data(
            vec!["a".to_string(), "b".to_string()],
            vec![vec!["1".to_string(), "2".to_string()]],
        );
        grid.hide_column(0);
        assert_eq!(grid.hidden.len(), 1);
        grid.hide_column(1);
        assert_eq!(grid.hidden.len(), 1, "只剩 1 列时再藏被拒绝");
        grid.show_all_columns();
        assert!(grid.hidden.is_empty(), "显示全部列清空隐藏清单");
    }

    /// 【P2-4】单行转置：选中行竖排成「# / 字段 / 值」，行导航夹取，
    /// 只读（不脏 / 不冲突 / 不进编辑），关闭后原网格原样回来
    #[test]
    fn transpose_shows_one_row_as_field_value_pairs() {
        let mut grid = ResultGridDelegate::default();
        grid.set_data(
            vec!["id".to_string(), "name".to_string(), "blob".to_string()],
            vec![
                vec!["1".to_string(), "alice".to_string(), "aabb".to_string()],
                vec!["2".to_string(), "bob".to_string(), "ccdd".to_string()],
            ],
        );
        // 竖排第 2 行（数据行序 1）
        grid.set_transposed(true, 1);
        assert!(grid.is_transposed());
        assert_eq!(grid.transpose_row(), RowRef::Data(1));
        // 三行 = 三个可见字段
        assert_eq!(grid.transpose_columns(), vec![0, 1, 2]);
        assert_eq!(grid.transpose_cell(0, 1), Some("id"));
        assert_eq!(grid.transpose_cell(0, 2), Some("2"), "第 2 行的 id 值");
        assert_eq!(grid.transpose_cell(1, 1), Some("name"));
        assert_eq!(grid.transpose_cell(1, 2), Some("bob"));
        assert_eq!(grid.transpose_cell(2, 2), Some("ccdd"));
        assert_eq!(grid.row_text(0), "id\t2", "转置一行 = 字段名 + 值");
        // 只读视图：不脏、不冲突、不进编辑（菜单短路在 context_menu 方法）
        assert!(!grid.is_dirty(0, 2));
        assert!(!grid.is_conflict(0, 2));
        assert!(!grid.is_editing(0, 2));
        assert!(!grid.begin_edit(0, 2));
        // 行导航：越界夹回最后一行
        grid.set_transpose_row(99);
        assert_eq!(grid.transpose_row(), RowRef::Data(1));
        // 关闭：原网格原样（隐藏列也没被改动）
        grid.set_transposed(false, 0);
        assert!(!grid.is_transposed());
        assert_eq!(grid.row_text(0), "1\talice\taabb", "普通视图回来");
    }

    /// 【P2-4】转置只投影可见列（隐藏列不竖排出来）
    #[test]
    fn transpose_skips_hidden_columns() {
        let mut grid = ResultGridDelegate::default();
        grid.set_data(
            vec!["id".to_string(), "name".to_string(), "blob".to_string()],
            vec![vec!["1".to_string(), "alice".to_string(), "aabb".to_string()]],
        );
        grid.hide_column(1);
        grid.set_transposed(true, 0);
        assert_eq!(grid.transpose_columns(), vec![0, 2], "隐藏的 name 不参与转置");
        assert_eq!(grid.transpose_cell(1, 1), Some("blob"));
        assert_eq!(grid.transpose_cell(1, 2), Some("aabb"));
    }

    /// 【P2-4】换数据自动退出转置（行与列都换了，旧转置行无意义）
    #[test]
    fn transpose_resets_when_data_changes() {
        let mut grid = ResultGridDelegate::default();
        grid.set_data(
            vec!["a".to_string(), "b".to_string()],
            vec![vec!["1".to_string(), "2".to_string()]],
        );
        grid.set_transposed(true, 0);
        assert!(grid.is_transposed());
        grid.set_data(vec!["x".to_string()], vec![vec!["9".to_string()]]);
        assert!(!grid.is_transposed(), "换结果自动退出转置");
    }

    // ===== 【#28-B2】本地叠加层行身份分叉 =====

    /// 造一行本地行快照（cells 与列同序；id 由叠加层取号）
    fn local_snapshot(cells: Vec<String>) -> LocalRowSet {
        let mut local = LocalRowSet::new();
        let id = local.next_id();
        local.push_local(LocalRow::new(id, cells));
        local
    }

    /// 视图序列 = 底层行按原序在前、本地行登记序追加末尾；身份与取值分叉正确，
    /// 本地行不占底层行号。
    #[test]
    fn local_rows_append_after_data_rows_and_resolve_identity() {
        let mut grid = grid_with(&[&["1", "alice"], &["2", "bob"]]);
        grid.set_local_snapshot(Some(local_snapshot(vec![
            "3".to_string(),
            "zoe".to_string(),
        ])));

        assert_eq!(grid.view_len(), 3, "底层 2 行 + 本地 1 行");
        assert_eq!(grid.data_row_count(), 2, "底层总数不变");
        assert_eq!(grid.row_ref(0), Some(RowRef::Data(0)));
        assert_eq!(grid.row_ref(1), Some(RowRef::Data(1)));
        assert_eq!(grid.row_ref(2), Some(RowRef::Local(1)));
        assert_eq!(grid.data_row(2), None, "本地行没有底层行号");

        // 取值分叉：本地行从快照取，不串底层行
        assert_eq!(grid.cell(2, 1), "3");
        assert_eq!(grid.cell(2, 2), "zoe");
        assert_eq!(grid.row_text(2), "3\tzoe", "整行 TSV 纳入本地行");
        assert!(
            grid.row_json_text(2).contains("zoe"),
            "行 JSON 纳入本地行"
        );
    }

    /// 本地行参与全局词 / 列级筛选（命中才出现）；不参与列排序，始终登记序追加末尾。
    #[test]
    fn local_rows_match_filters_but_are_excluded_from_sorting() {
        let mut grid = grid_with(&[&["1", "alice"], &["2", "bob"], &["3", "carol"]]);
        grid.set_local_snapshot(Some(local_snapshot(vec![
            "9".to_string(),
            "zoe".to_string(),
        ])));

        // 排序（按 id 降序）只作用底层行；本地行保持登记序追加末尾
        grid.apply_sort(Some((0, true)));
        assert_eq!(grid.row_ref(0), Some(RowRef::Data(2)));
        assert_eq!(grid.row_ref(1), Some(RowRef::Data(1)));
        assert_eq!(grid.row_ref(2), Some(RowRef::Data(0)));
        assert_eq!(
            grid.row_ref(3),
            Some(RowRef::Local(1)),
            "本地行不参与排序，落在末尾"
        );

        // 全局词命中本地行 → 本地行单独出现；底层不命中
        grid.apply_sort(None);
        grid.set_filter("zoe");
        assert_eq!(grid.view_len(), 1);
        assert_eq!(grid.row_ref(0), Some(RowRef::Local(1)));

        // 列级筛选同理（列 1 = name）
        grid.set_filter("");
        grid.set_column_filter(Some((1, "zoe".to_string())));
        assert_eq!(grid.view_len(), 1);
        assert_eq!(grid.row_ref(0), Some(RowRef::Local(1)));
    }

    /// 本地行不脏、不冲突：格级 ChangeSet 只管底层行；本地行格编辑走本地快照钩子，
    /// 不进待提交变更集与格级冲突集合。
    #[test]
    fn local_rows_are_never_dirty_or_conflict() {
        let mut grid = grid_with(&[&["1", "alice"]]);
        grid.set_local_snapshot(Some(local_snapshot(vec![
            String::new(),
            "zoe".to_string(),
        ])));
        assert!(!grid.is_dirty(1, 1));
        assert!(!grid.is_conflict(1, 1));
    }

    /// 【#28-B4】本地行身份辅助：`row_is_local` 标本地行，`is_deleted_row` 只认被删底层行；
    /// 编辑闸口：editable=false 时本地行也不进编辑；editable=true 时本地行可进入本地编辑态。
    #[test]
    fn local_row_identity_and_edit_gate() {
        let mut grid = grid_with(&[&["1", "alice"], &["2", "bob"]]);
        // 标记底层第 1 行（视图行号 1）为待删除；本地行追加到视图末位
        let mut local = LocalRowSet::new();
        local.mark_deleted(1);
        let id = local.next_id();
        local.push_local(LocalRow::new(id, vec!["3".to_string(), "zoe".to_string()]));
        grid.set_local_snapshot(Some(local));

        assert!(grid.row_is_local(2), "视图末位 = 本地行");
        assert!(!grid.row_is_local(0));
        assert!(grid.is_deleted_row(1), "底层第 2 行被标记删除");
        assert!(!grid.is_deleted_row(0));
        assert!(!grid.is_deleted_row(2), "本地行永不标删除");

        // 关闭编辑闸口：本地行与被删行都不进编辑
        assert!(!grid.begin_edit(2, 1));
        assert!(!grid.begin_edit(1, 1));

        // 打开闸口：本地行可进入；被删底层行仍拒绝（撤销删除后才恢复可编辑）
        grid.set_editable(true);
        assert!(grid.begin_edit(2, 1), "本地行双击进入本地编辑态");
        assert!(!grid.begin_edit(1, 1), "被删行拒绝进入编辑");
        assert!(grid.is_editing(2, 1));
    }

    /// 【#28-B4】本地行格编辑提交走本地钩子：收到本地行 id / 列 / 新值，不触发底层
    /// 变更集钩子；值没变不落钩子。钩子为空时空安全，不 panic。
    #[test]
    fn local_row_edit_commit_branches_to_local_hook() {
        let mut grid = grid_with(&[&["1", "alice"]]);
        grid.set_editable(true);
        let mut local = LocalRowSet::new();
        let id = local.next_id();
        local.push_local(LocalRow::new(id, vec!["3".to_string(), "zoe".to_string()]));
        grid.set_local_snapshot(Some(local));

        // 记录本地钩子入参；底层钩子被调 = 测试失败（不进变更集路径）
        let seen = Rc::new(std::cell::RefCell::new(Vec::<(u64, usize, String)>::new()));
        let seen_for_hook = seen.clone();
        grid.set_commit_local_edit_hook(Rc::new(move |local_id, column, value| {
            seen_for_hook.borrow_mut().push((local_id, column, value));
        }));
        let data_called = Rc::new(std::cell::RefCell::new(false));
        let data_for_hook = data_called.clone();
        grid.set_commit_edit_hook(Rc::new(move |_row, _col, _orig, _edited| {
            *data_for_hook.borrow_mut() = true;
        }));

        // 本地行改值 → 走本地钩子
        assert!(grid.begin_edit(1, 2));
        grid.sync_edit_value("zoe2".to_string());
        assert!(grid.commit_edit(), "类型校验通过即提交");
        assert_eq!(
            seen.borrow().as_slice(),
            &[(id, 1, "zoe2".to_string())],
            "本地钩子收到本地行 id / 数据列 / 新值"
        );
        assert!(!*data_called.borrow(), "底层变更集钩子未被调用");

        // 值没变（改回原值）→ 不落本地钩子。纯 delegate 快照不联动钩子回写，这里在
        // 原始快照「zoe」上验证 original == value 分支，不模拟宿主重投影后的新初值。
        seen.borrow_mut().clear();
        assert!(grid.begin_edit(1, 2));
        grid.sync_edit_value("zoe".to_string());
        assert!(grid.commit_edit());
        assert!(seen.borrow().is_empty(), "原值 == 新值不写本地快照");
    }

    /// 转置锚定本地行时从本地快照取值（#28-B1 拍板 D2：转置纳入本地行）。
    #[test]
    fn transpose_reads_local_row_cells_when_anchored_to_local() {
        let mut grid = grid_with(&[&["1", "alice"]]);
        grid.set_local_snapshot(Some(local_snapshot(vec![
            "9".to_string(),
            "zoe".to_string(),
        ])));
        grid.set_transposed(true, 1);
        assert_eq!(grid.transpose_row(), RowRef::Local(1));
        assert_eq!(grid.transpose_cell(0, 1), Some("id"));
        assert_eq!(grid.transpose_cell(0, 2), Some("9"), "本地行 id 列快照值");
        assert_eq!(grid.transpose_cell(1, 2), Some("zoe"));
    }

    /// 本地行不下发 INSERT：行级新增本期不进提交批次（#28-B1 排除矩阵）。
    #[test]
    fn insert_text_skips_local_rows() {
        let mut grid = grid_with(&[&["1", "alice"]]);
        grid.set_result_table(Some("orders".to_string()));
        grid.set_local_snapshot(Some(local_snapshot(vec![
            "9".to_string(),
            "zoe".to_string(),
        ])));
        assert!(
            grid.row_insert_text(1).is_empty(),
            "本地行不拼 INSERT"
        );
        assert!(
            !grid.row_insert_text(0).is_empty(),
            "底层行照常拼 INSERT"
        );
    }

    // ===== 【#28-B5】全链路排除本地行 / 被删行（排除矩阵收口） =====

    /// 构造：底层 3 行，标记底层第 2 行（Data(1)，bob）删除，追加 1 个本地行（zoe）。
    fn grid_with_deleted_and_local() -> ResultGridDelegate {
        let mut grid = grid_with(&[&["1", "alice"], &["2", "bob"], &["3", "carol"]]);
        let mut local = LocalRowSet::new();
        local.mark_deleted(1);
        let id = local.next_id();
        local.push_local(LocalRow::new(
            id,
            vec!["9".to_string(), "zoe".to_string()],
        ));
        grid.set_local_snapshot(Some(local));
        grid
    }

    /// 文本视图序列：底层未删行 + 本地行；被删行排除（拍板 D2）。
    #[test]
    fn text_view_sequence_keeps_local_rows_and_drops_deleted() {
        let rows = grid_with_deleted_and_local().visible_rows();
        assert_eq!(rows.len(), 3, "底层未删 2 行 + 本地 1 行");
        assert_eq!(rows[0], vec!["1", "alice"]);
        assert_eq!(rows[1], vec!["3", "carol"]);
        assert_eq!(rows[2], vec!["9", "zoe"], "本地行纳入文本视图");
    }

    /// 导出序列：只含底层未删行；本地行与被删行都排除。
    #[test]
    fn export_sequence_drops_both_local_and_deleted_rows() {
        let rows = grid_with_deleted_and_local().visible_rows_for_export();
        assert_eq!(rows, vec![vec!["1", "alice"], vec!["3", "carol"]]);
    }

    /// 统计 / 分组取值（column_values）：本地排除、被删排除（D9 本期口径）。
    #[test]
    fn column_values_excludes_local_and_deleted_rows() {
        let grid = grid_with_deleted_and_local();
        assert_eq!(grid.column_values(0), vec!["1", "3"]);
        assert_eq!(grid.column_values(1), vec!["alice", "carol"]);
    }

    /// 工具栏整份复制（active_result_tsv）：本地纳入、被删排除；计数同口径（拍板 D7）。
    #[test]
    fn active_result_copy_keeps_local_rows_and_drops_deleted() {
        let (text, count) = grid_with_deleted_and_local().active_result_tsv();
        assert_eq!(count, 3, "底层未删 2 行 + 本地 1 行");
        assert!(text.contains("alice"));
        assert!(text.contains("carol"));
        assert!(text.contains("zoe"), "本地行进整份复制");
        assert!(!text.contains("bob"), "被删行不进整份复制");
    }

    /// 复制列：本地纳入、被删排除。
    #[test]
    fn column_text_keeps_local_and_drops_deleted() {
        let grid = grid_with_deleted_and_local();
        assert_eq!(grid.column_text(1), "alice\ncarol\nzoe");
    }

    /// 被删行整行 TSV / 行 JSON / 选中复制 / INSERT 全部落空（不拷走被删行）。
    #[test]
    fn copy_text_rejects_deleted_rows() {
        let grid = grid_with_deleted_and_local();
        assert!(grid.row_text(1).is_empty(), "被删行整行 TSV 为空");
        assert!(grid.row_json_text(1).is_empty(), "被删行行 JSON 为空");
        assert_eq!(
            grid.selection_text(GridSelection::Row { row: 1 }),
            None,
            "被删行整行复制返回 None"
        );
        assert!(grid.row_insert_text(1).is_empty(), "被删行不拼 INSERT");
    }

    // ===== 【B5】事实档列式保真分流 =====

    /// 工具栏载体段：内存档报列式占用，表档只标载体，无事实档不摆段。
    #[test]
    fn toolbar_segments_show_fact_tier_and_occupancy() {
        let memory = ResultToolbar {
            rows: Some(2),
            fact_tier: Some(CurrentTier::InMemory),
            fact_bytes: 1_258_291,
            ..Default::default()
        };
        assert_eq!(memory.segments(), ["行数 2", "内存档 1.2 MiB"]);

        let table = ResultToolbar {
            rows: Some(2),
            fact_tier: Some(CurrentTier::OnDuckDb),
            fact_bytes: 0,
            ..Default::default()
        };
        assert_eq!(table.segments(), ["行数 2", "表档"]);

        let none = ResultToolbar { rows: Some(2), ..Default::default() };
        assert_eq!(none.segments(), ["行数 2"]);
    }

    /// 【B5】内存档数值列统计走事实档原生聚合：NULL 判定来自事实帧，不看矩阵文本。
    #[test]
    fn column_stats_uses_fact_native_numeric_when_present() {
        let fact = fact_frame(
            &["id", "name"],
            vec![
                Arc::new(Int64Array::from(vec![Some(3), None, Some(7)])),
                Arc::new(StringArray::from(vec![Some("a"), Some("b"), Some("c")])),
            ],
        );
        let mut grid = ResultGridDelegate::empty("尚未执行");
        grid.set_data(
            vec!["id".to_string(), "name".to_string()],
            vec![
                vec!["3".to_string(), "a".to_string()],
                // 矩阵第二格写失真文本：事实档该行为 NULL，统计应以事实帧为准
                vec!["missing".to_string(), "b".to_string()],
                vec!["7".to_string(), "c".to_string()],
            ],
        );
        grid.set_fact(Some(fact));
        let stats = grid.column_stats_for(0);
        assert_eq!(stats.count, 2, "事实档非空两行");
        assert_eq!(stats.nulls, 1, "NULL 来自事实档，不被矩阵文本误判成非空");
        assert_eq!(stats.min, Some(3.0));
        assert_eq!(stats.max, Some(7.0));
        assert_eq!(stats.sum, Some(10.0));
        assert_eq!(stats.avg, Some(5.0));
    }

    /// 【B5】事实档文本列回退矩阵口径：只计非空，不填数值字段。
    #[test]
    fn column_stats_falls_back_to_matrix_for_text_fact_column() {
        let fact = fact_frame(
            &["name"],
            vec![Arc::new(StringArray::from(vec![Some("a"), Some("b")]))],
        );
        let mut grid = ResultGridDelegate::empty("尚未执行");
        grid.set_data(
            vec!["name".to_string()],
            vec![vec!["a".to_string()], vec!["b".to_string()]],
        );
        grid.set_fact(Some(fact));
        let stats = grid.column_stats_for(0);
        assert_eq!(stats.count, 2);
        assert_eq!(stats.nulls, 0);
        assert_eq!(stats.min, None);
    }

    /// 【B5】表档帧 batches 已释放：统计回退显示矩阵，不回读 Rust 堆。
    #[test]
    fn column_stats_backs_off_to_matrix_when_fact_released() {
        let mut fact = fact_frame(
            &["id"],
            vec![Arc::new(Int64Array::from(vec![Some(3), Some(7)]))],
        );
        Arc::get_mut(&mut fact).expect("测试独占事实帧").release_batches();
        let mut grid = ResultGridDelegate::empty("尚未执行");
        grid.set_data(
            vec!["id".to_string()],
            vec![vec!["3".to_string()], vec!["7".to_string()]],
        );
        grid.set_fact(Some(fact));
        let stats = grid.column_stats_for(0);
        assert_eq!(stats.count, 2, "矩阵口径两行非空");
        assert_eq!(stats.min, Some(3.0));
    }

    /// 【B5】内存档数值列分组按原生值归一化键分组：整数统一写成整数字面量，
    /// 避免 `1.0` 与 `1` 被字符串等值拆成两组。
    #[test]
    fn group_by_fact_normalizes_integer_keys() {
        let fact = fact_frame(
            &["price"],
            vec![Arc::new(Float64Array::from(vec![Some(1.0), Some(2.0), Some(1.0)]))],
        );
        let mut grid = ResultGridDelegate::empty("尚未执行");
        grid.set_data(
            vec!["price".to_string()],
            vec![
                vec!["1.0".to_string()],
                vec!["2.0".to_string()],
                vec!["1.0".to_string()],
            ],
        );
        grid.set_fact(Some(fact));
        assert_eq!(
            grid.group_by_for(0),
            vec![("1".to_string(), 2), ("2".to_string(), 1)],
            "整数归一化后 1.0 与 1 同组"
        );
    }

    /// 【B5】内存档数值列排序走原生数值比较（回归保护：事实档存在时排序仍正确）。
    #[test]
    fn sort_fact_orders_native_numeric() {
        let fact = fact_frame(
            &["id"],
            vec![Arc::new(Int64Array::from(vec![Some(10), Some(2)]))],
        );
        let mut grid = ResultGridDelegate::empty("尚未执行");
        grid.set_data(
            vec!["id".to_string()],
            vec![vec!["10".to_string()], vec!["2".to_string()]],
        );
        grid.set_fact(Some(fact));
        grid.apply_sort(Some((0, false)));
        assert_eq!(grid.visible_rows()[0][0], "2");
        assert_eq!(grid.visible_rows()[1][0], "10");
    }

    /// 【基准】文本视图 TSV 拼接：首拼全量 50k 行 ~120ms，指纹缓存命中后应接近 0
    /// （hot 必须 < cold/10：缓存是文本视图切换流畅性的核心，防回归）。
    #[test]
    fn bench_text_presentation_large_set() {
        let mut grid = ResultGridDelegate::default();
        let cols: Vec<String> = (0..8).map(|i| format!("col{}", i)).collect();
        let rows: Vec<Vec<String>> = (0..50_000)
            .map(|r| (0..8).map(|c| format!("v{}:{}", r, c)).collect())
            .collect();
        grid.set_data(cols, rows);
        let t0 = std::time::Instant::now();
        let text = grid.text_presentation();
        let cold = t0.elapsed();
        let t1 = std::time::Instant::now();
        let cached = grid.text_presentation();
        let hot = t1.elapsed();
        eprintln!(
            "BENCH text_presentation 50k x 8: cold={:?} hot={:?} len={}",
            cold,
            hot,
            text.len()
        );
        assert_eq!(text, cached, "同指纹命中缓存，文本一致");
        assert!(
            hot < cold / 10,
            "缓存命中应远快于首拼（cold={cold:?} hot={hot:?}）"
        );
    }

//! `grid/` 全量单元测试（2026-10-29 拆分时自 `crate::grid` 末尾 `mod tests` 整体迁入）。
//! 安全模式：不通配导入（`#[gpui_kit::test]` 与 `use gpui_kit::*` 会自相残杀）。

use crate::grid::cell::{
    ViewerMode, blob_summary, cell_display_text, cell_style, classify_value, clip_cell_display,
    detect_image_format, hex_lines, image_format_of, json_tree_items, pretty_json_display,
};
use crate::grid::header::header_tooltip_lines;
use crate::grid::stats::{distinct_count, fmt_grouped, fmt_stat_num, median_of, mode_of, text_minmax};
use crate::grid::status::{
    ResultControls, format_integer_cell, render_status_row, render_toolbar,
};
use crate::grid::status::{FIRST_PAGER_ACTION, FIRST_SYSTEM_ACTION};
use crate::grid::text::{cell_matches, find_in_cell, preview_of};
use crate::grid::view_rows::compare_cells;
use crate::ports::{FindCriteria, Presentation};
use gpui_kit::ImageFormat;
use crate::grid::{
    CELL_DISPLAY_LIMIT, ContextAction, ContextTarget, GridSelection, MAX_SINGLE_SEGMENT_ROWS,
    ResultGridDelegate, ResultMatch, ResultStatus, ResultToolbar, SelectionStatKind,
    SelectionStatPrefs, WINDOW_FETCH_NOTICE, can_edit_cell, column_browser_menu_items,
    column_distinct_values,
    column_menu_items, column_stats, column_stats_from_values, context_menu_items, duration_text,
    fetch_more_hint, filter_panel_menu_item, filter_settings_menu_items, filters_menu_items,
    format_cell_text,
    group_by_values, layout_presentation_menu_items, reset_cell_applicable, row_action_state,
    row_number_menu_items, row_target_state,
    selection_stat_menu_items, selection_statistics, stats_lines, status_action_placeholders,
    status_segments, thousands, truncated_hint, truncation_notice, validate_edit_value,
    window_fetch_active,
};
use crate::rowset::{LocalRow, LocalRowSet, RowRef};
use crate::store::{ColumnKind, ColumnType};
use ::shared::{CurrentTier, FactFrame};
use arrow::array::{ArrayRef, Float64Array, Int64Array, StringArray};
use arrow::datatypes::{Field, Schema};
use arrow::record_batch::RecordBatch;
use gpui_kit::App;
use gpui_kit::TextAlign;
use gpui_kit::assets::IconName;
use gpui_kit::component::table::{ColumnSort, TableDelegate as _};
use gpui_kit::component::Sizable as _;
use gpui_kit::{AppContext as _, ParentElement as _, Styled as _};
use std::rc::Rc;
use std::sync::Arc;
use workbench_shell::model::ColumnAnnotation;
use workbench_shell::result_style::{AlignPref, ColorRolePref, TypeStylePrefs};

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
        FactFrame::new(columns.iter().map(|n| n.to_string()).collect(), vec![batch])
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
        analyze: false,
        persist: false,
        table: None,
        column_filter_active: false,
        column_values: Vec::new(),
        sort_active: false,
        row_ref: RowRef::default(),
        // 默认只读：既有菜单断言不摆行级三项；可编辑用例在测试里显式覆盖
        editable: false,
        is_deleted: false,
        cell_changed: false,
        // 【选中统计】三项开关默认全关（出厂态，`DataEditorsPreferencesInitializer.java:86-88`）
        selection_stat_rows: false,
        selection_stat_columns: false,
        selection_stat_cells: false,
        // 【布局段】默认网格态、记录模式关（面板出厂态）；筛选条出厂显示
        presentation: Presentation::Grid,
        record_mode: false,
        show_filter_panel: true,
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
    assert_eq!(
        detect_image_format("524946460000000057454250"),
        Some("WebP")
    );
    assert_eq!(detect_image_format("00000000"), None);
    assert_eq!(detect_image_format(""), None);
    // 非 hex 文本不误判
    assert_eq!(detect_image_format("hello world"), None);
}

/// 图片魔数 → gpui 图片格式：四个支持格式都映射，未登记的名字 → None
/// （值查看器据此决定摆 `img` 还是回文本占位）
#[test]
fn image_format_of_maps_recognized_magic_names() {
    assert_eq!(image_format_of("PNG"), Some(ImageFormat::Png));
    assert_eq!(image_format_of("JPEG"), Some(ImageFormat::Jpeg));
    assert_eq!(image_format_of("GIF"), Some(ImageFormat::Gif));
    assert_eq!(image_format_of("WebP"), Some(ImageFormat::Webp));
    assert_eq!(image_format_of("BMP"), None);
    assert_eq!(image_format_of(""), None);
    // 端到端：魔数识别 → 格式映射，两段必须都能接上（断链会静默退回文本占位）
    assert_eq!(
        detect_image_format("89504e470d0a1a0a").and_then(image_format_of),
        Some(ImageFormat::Png)
    );
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
    let items = json_tree_items(r#"{"user":{"name":"a","tags":["x","y"]},"n":1}"#)
        .expect("合法 JSON 应建树");
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
    let n = root
        .children
        .iter()
        .find(|c| c.id == "$.n")
        .expect("n 子项");
    assert!(!n.is_folder()); // 标量叶子
    assert_eq!(n.label, "n: 1");
}

/// JSON 树：超大数组截断 + 非法 JSON → None
#[test]
fn json_tree_items_truncates_large_arrays_and_rejects_non_json() {
    // 超过 JSON_TREE_CHILD_LIMIT 的数组 → 截断 + 「… 还有 N 项」占位
    let big = format!(
        "[{}]",
        (0..700)
            .map(|i| i.to_string())
            .collect::<Vec<_>>()
            .join(",")
    );
    let items = json_tree_items(&big).expect("大数组应建树");
    let root = &items[0];
    assert_eq!(
        root.children.len(),
        crate::grid::cell::JSON_TREE_CHILD_LIMIT + 1
    );
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
            // 【DBeaver 对齐】`fillContextMenu:3166-3177` 复制段之后挂 filters 子菜单，
            // 本仓扁平化后 = `fillFiltersMenu:3864-3933` 的整组（首项查找 + 四项设置，见 §9.37）
            "在结果中查找…",
            "保存过滤器设置",
            "重置过滤器设置",
            "删除所有过滤 / 排序",
            "编辑筛选设置…",
            "按「name」升序（下发源库）",
            "按「name」降序（下发源库）",
            "冻结「name」",
            "隐藏「name」",
            "列统计「name」",
            // 【布局段】呈现方式组（记录 / 切换表示 / 网格·文本）+ 选中统计三颗，
            // 两段顺序照 `ResultSetViewer.fillLayoutMenu:3549-3597`（本仓无子菜单，见 §9.33）
            "记录",
            "切换表示",
            "✓ 网格",
            "文本",
            // 【选中统计】layout 段三颗开关平铺在右键尾部（本仓无子菜单，见 §9.32）
            "显示选定的行数",
            "显示选定的列数",
            "显示选定的单元格数",
            // 【筛选条显隐】统计三颗之后（DBeaver `fillLayoutMenu:3589-3592`，见 §9.34）
            "✓ 显示过滤器面板",
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
    assert_eq!(
        items[2].action,
        ContextAction::CopyValue("orders".to_string())
    );
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
    assert_eq!(items[7].action, ContextAction::OpenFind);
    assert_eq!(
        items[12].action,
        ContextAction::SortDown {
            column: "name".to_string(),
            descending: false
        }
    );
    assert_eq!(
        items[13].action,
        ContextAction::SortDown {
            column: "name".to_string(),
            descending: true
        }
    );
    assert!(
        items[7].separator_before,
        "filters 段（首项 = 在结果中查找…）之前要有分隔线（DBeaver `fillContextMenu:3166`）"
    );
    assert!(
        !items[12].separator_before,
        "排序段紧跟 filters 段、首项不带分隔线（DBeaver order 子菜单在 filters 之后且内部无 \
         Separator，`fillContextMenu:3179-3187` / `fillOrderingsMenu:3939-3946`）"
    );
    assert!(
        items[14].separator_before,
        "列级视图段（冻结起）与上方行级动作段之间要有一条分隔线（DBeaver `:3219` 的 Separator）"
    );
    assert_eq!(items[14].action, ContextAction::ToggleFreeze(1));
    assert_eq!(
        items[15].action,
        ContextAction::HideColumn(1),
        "【P2】隐藏此列跟在冻结后（都是列级动作）"
    );
    assert_eq!(
        items[16].action,
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
    assert_eq!(items[14].label, "取消冻结此列", "已冻结时给的是反向动作");

    // 【P2】隐藏了列才出现「显示全部列」
    let mut hidden = target.clone();
    hidden.hidden_columns = 2;
    let items = context_menu_items(&hidden);
    assert_eq!(
        items[17].action,
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

/// 【#28-B3】行级动作（新增 / 复制 / 删除一行 + 第 55 批的「从上一行 / 下一行复制」）的菜单闸口：
/// - 只读结果集（`!editable`）整组不摆；
/// - 已标记删除的底层行五项全不摆；
/// - 可编辑且未删时五项都摆：前三项一组（新增项带分隔线）、后两项同组（首项带分隔线，
///   对应 DBeaver `fillContextMenu:3212` 的段内 Separator），整体排在**排序段之后**
///   （DBeaver 段序 filters → order → navigate → edit，`fillContextMenu:3169-3218`）；
/// - 动作携带视图行号（与复制整行同口径，不是列下标）。
#[test]
fn context_menu_row_actions_gate_on_editable_and_deleted() {
    // 只读：五项都不摆
    let read_only = context_menu_items(&target("orders", "name", 1));
    assert!(
        !read_only.iter().any(|item| matches!(
            item.action,
            ContextAction::InsertRowBelow(_)
                | ContextAction::DuplicateRow(_)
                | ContextAction::DeleteRow(_)
                | ContextAction::CopyRowValuesFrom { .. }
        )),
        "只读结果集不摆行级动作"
    );

    // 可编辑、未删：五项都摆
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
    let from_above = items
        .iter()
        .find(|item| matches!(item.action, ContextAction::CopyRowValuesFrom { above: true, .. }))
        .expect("可编辑结果集要摆「从上一行复制」");
    let from_below = items
        .iter()
        .find(|item| matches!(item.action, ContextAction::CopyRowValuesFrom { above: false, .. }))
        .expect("可编辑结果集要摆「从下一行复制」");
    assert_eq!(insert.action, ContextAction::InsertRowBelow(0));
    assert_eq!(duplicate.action, ContextAction::DuplicateRow(0));
    assert_eq!(delete.action, ContextAction::DeleteRow(0));
    assert_eq!(
        from_above.action,
        ContextAction::CopyRowValuesFrom {
            row: 0,
            above: true
        }
    );
    assert_eq!(
        from_below.action,
        ContextAction::CopyRowValuesFrom {
            row: 0,
            above: false
        }
    );
    assert!(
        insert.separator_before,
        "行级动作独立分组：新增项前带分隔线（第 61 批后这条是 edit 段内第二条，\
         第一条在「修改单元格」上、对应 DBeaver `:3208`）"
    );
    assert!(!duplicate.separator_before, "复制 / 删除与新增同组");
    assert!(!delete.separator_before, "复制 / 删除与新增同组");
    assert!(
        from_above.separator_before,
        "「从上一行复制」前一条段内分隔线（DBeaver `fillContextMenu:3212`），
         把「改行本身」与「借用邻行的值」两组分开"
    );
    assert!(!from_below.separator_before, "两个邻行复制项同组");

    // 分组位置：复制段（CopyRowJson / CopyRowInsert）之后、**排序段之后**
    // （DBeaver 的 order 子菜单在 filters 之后、edit 之前，`:3179-3218`）
    let row_action_indexes = [insert, duplicate, delete, from_above, from_below]
        .iter()
        .map(|item| {
            items
                .iter()
                .position(|candidate| candidate.action == item.action)
        })
        .collect::<Option<Vec<_>>>()
        .expect("五项都在菜单里");
    let first_row_action = *row_action_indexes.iter().min().unwrap();
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
    let last_sort = items
        .iter()
        .rposition(|item| matches!(
            item.action,
            ContextAction::SortDown { .. } | ContextAction::ClearSort
        ))
        .expect("排序段至少有一项");
    assert!(last_copy < first_sort, "排序段排在复制段之后");
    assert!(
        last_sort < first_row_action,
        "行级动作排在排序段之后（DBeaver edit 子菜单在 order 之后，`:3197-3218`）"
    );
    // 邻行复制两项在「删除此行」之后（DBeaver `:3211` DELETE → `:3212` Separator → `:3213-3214`）
    let delete_index = row_action_indexes[2];
    assert!(
        delete_index < row_action_indexes[3] && row_action_indexes[3] < row_action_indexes[4],
        "「从上一行复制 / 从下一行复制」紧跟「删除此行」之后"
    );

    // 【第 65 批】已删除行：DBeaver 是「项在、灰掉」而不是整段收起
    // （`fillContextMenu:3209-3211` 无条件 add，能不能点由 `PROP_EDITABLE` 决定）。
    // 五项都仍在菜单里，只有「删除此行」因本仓收紧（已删行再删是空操作）而 disabled。
    let mut deleted = target("orders", "name", 1);
    deleted.editable = true;
    deleted.is_deleted = true;
    let deleted_items = context_menu_items(&deleted);
    let row_level = deleted_items
        .iter()
        .filter(|item| {
            matches!(
                item.action,
                ContextAction::InsertRowBelow(_)
                    | ContextAction::DuplicateRow(_)
                    | ContextAction::DeleteRow(_)
                    | ContextAction::CopyRowValuesFrom { .. }
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(row_level.len(), 5, "已删行上行级五项仍在菜单里");
    for item in &row_level {
        if matches!(item.action, ContextAction::DeleteRow(_)) {
            assert!(item.disabled, "已删行的「删除此行」置灰（本仓收紧，§9.48）");
        } else {
            assert!(!item.disabled, "「{}」在已删行上仍可用", item.label);
        }
    }
}

/// 【第 65 批】`ContextMenuItem::disabled` 是**新增维度**：默认一律可用，只有明确判出
/// 「这一项对当前落点不成立」时才灰。这条用例守住默认值——新加菜单项若忘了写 disabled，
/// 编译会报（结构体字面量必填），但若有人为了过编译随手填 `true`，这里会炸。
#[test]
fn context_menu_items_are_enabled_unless_explicitly_shaded() {
    let mut editable = target("orders", "name", 1);
    editable.editable = true;
    let items = context_menu_items(&editable);
    for item in &items {
        assert!(
            !item.disabled,
            "可编辑未删行的右键菜单里不应有灰项：{}",
            item.label
        );
    }
}

/// 【第 61 批】右键「修改单元格」：对齐 DBeaver `fillEditMenu:3518-3525`——edit 子菜单里
/// 最先摆 `CMD_ROW_EDIT` / `CMD_ROW_EDIT_INLINE`（门控 `valueController != null` 且
/// `!isReadOnly()`），之后 `:3208` 再来一条 Separator 才接行级 ADD/COPY/DELETE。
///
/// 本仓扁平化后的对应形态：`EditCell` 是 edit 段**首项**且在「在下方插入新行」之前，
/// 这两项各自带前导分隔线（第一条开段、第二条是 DBeaver 那条段内 Separator）。
/// 只读 / 记录视图整段不摆；已删行整段**仍在**，只是「修改单元格」灰着（第 65 批）。
#[test]
fn context_menu_edit_cell_leads_the_edit_group() {
    // 只读：没有「修改单元格」
    let read_only = context_menu_items(&target("orders", "name", 1));
    assert!(
        !read_only
            .iter()
            .any(|item| matches!(item.action, ContextAction::EditCell { .. })),
        "只读结果集不摆「修改单元格」"
    );

    // 可编辑未删：摆在 edit 段首
    let mut editable = target("orders", "name", 1);
    editable.editable = true;
    let items = context_menu_items(&editable);
    let edit = items
        .iter()
        .position(|item| matches!(item.action, ContextAction::EditCell { .. }))
        .expect("可编辑结果集要摆「修改单元格」");
    assert_eq!(
        items[edit].action,
        ContextAction::EditCell {
            row: 0,
            // 右键 target 的 `column` 是**数据列下标**（已折算掉行号槽），出菜单时才 +1
            column: 1,
        }
    );
    assert_eq!(
        items[edit].label, "修改单元格",
        "文案沿用状态栏那颗（§9.24），不照 bundle_zh 的「内嵌编辑」"
    );
    assert!(items[edit].separator_before, "edit 段首项带开段分隔线");
    let insert = items
        .iter()
        .position(|item| matches!(item.action, ContextAction::InsertRowBelow(_)))
        .expect("可编辑结果集要摆新增行");
    assert!(edit < insert, "「修改单元格」在 edit 段首（先于行级三项）");
    assert!(
        items[insert].separator_before,
        "新增行前的分隔线 = DBeaver `:3208` 的段内 Separator"
    );

    // 【第 65 批】已删除行：项仍在，但灰着（`begin_edit` 直接拒绝已删行；
    // 第 67 批起旁边会多出「重置单元格」这条恢复通道，见
    // `reset_cell_item_appears_only_when_dirty_or_deleted`）
    let mut deleted = target("orders", "name", 1);
    deleted.editable = true;
    deleted.is_deleted = true;
    let deleted_items = context_menu_items(&deleted);
    let edit = deleted_items
        .iter()
        .position(|item| matches!(item.action, ContextAction::EditCell { .. }))
        .expect("已删行仍摆「修改单元格」（DBeaver 无条件 add，靠 enabled 判）");
    assert!(deleted_items[edit].disabled, "已删行的「修改单元格」置灰");

    // 【第 62 批】记录（整行）视图：`record_view` 这条并入统一判据后，整段编辑项都不摆
    let mut record = target("orders", "name", 1);
    record.editable = true;
    record.record_mode = true;
    let record_items = context_menu_items(&record);
    assert!(
        !record_items.iter().any(|item| matches!(
            item.action,
            ContextAction::EditCell { .. }
                | ContextAction::InsertRowBelow(_)
                | ContextAction::DuplicateRow(_)
                | ContextAction::DeleteRow(_)
                | ContextAction::CopyRowValuesFrom { .. }
        )),
        "记录视图是只读呈现（PlainTextPresentation 不实现 IResultSetEditor），编辑段整段不摆"
    );
}

/// 【第 67 批】`reset_cell_applicable` 的真值表。对齐 DBeaver `fillEditMenu:3536` 的门控
/// `row.getState() == STATE_REMOVED || row.isChanged(attr)`：
/// 只有「行已删」或「这一格改过」时才摆这一项。
#[test]
fn reset_cell_applicable_follows_the_dbeaver_gate() {
    // 未删未改：不摆（DBeaver `if` 不成立 → 菜单里没有这一项）
    assert!(!reset_cell_applicable(false, false));
    // 行已删：摆（重置 = 撤销删除标记，`resetCellValue:572-575`）
    assert!(reset_cell_applicable(true, false));
    // 这一格改过：摆（重置 = 按 `getChange` 回写原值，`:577-589`）
    assert!(reset_cell_applicable(false, true));
    // 行已删且格也改过：摆（第一支优先，行恢复后再谈格）
    assert!(reset_cell_applicable(true, true));
}

/// 【第 67 批】右键「重置单元格」是**条件出现**，不是灰项——这与第 65 批「项在、灰掉」
/// 的口径**有意不同**：DBeaver `fillEditMenu:3536` 是把 `CMD_CELL_RESET` 放进 `if`
/// 里条件 add，不成立时菜单里根本没有这一项，用户看不到「本来有东西」。本仓照抄，
/// 所以 `reset_cell_applicable` 为假时**不 push** 任何项（见 `menu.rs` 的 edit 段）。
#[test]
fn reset_cell_item_appears_only_when_dirty_or_deleted() {
    // 干净的可编辑行：没有「重置单元格」
    let mut clean = target("orders", "name", 1);
    clean.editable = true;
    let clean_items = context_menu_items(&clean);
    assert!(
        !clean_items
            .iter()
            .any(|item| matches!(item.action, ContextAction::ResetCell { .. })),
        "未删未改的格不摆「重置单元格」（条件 add，不是灰项）"
    );

    // 这一格改过：摆，且**紧跟**「修改单元格」（DBeaver `:3536` 在 ROW_EDIT 之后、
    // `:3208` Separator 之前，同组无分隔线）
    let mut dirty = target("orders", "name", 1);
    dirty.editable = true;
    dirty.cell_changed = true;
    let dirty_items = context_menu_items(&dirty);
    let edit_pos = dirty_items
        .iter()
        .position(|item| matches!(item.action, ContextAction::EditCell { .. }))
        .expect("可编辑结果集要摆「修改单元格」");
    let reset = dirty_items
        .get(edit_pos + 1)
        .expect("「修改单元格」后面要有「重置单元格」");
    assert_eq!(
        reset.action,
        ContextAction::ResetCell { row: 0, column: 1 },
        "落点与「修改单元格」同一格（`column` 是数据列下标，出菜单时 +1）"
    );
    assert_eq!(reset.label, "重置单元格", "文案对仗「修改单元格」（§9.24 口径）");
    assert!(!reset.separator_before, "与「修改单元格」同组，不插分隔线");
    assert!(!reset.disabled, "条件成立的项就是可点的");

    // 行已删（格没改）：也摆——这是「撤销这一行的删除」的入口
    let mut deleted = target("orders", "name", 1);
    deleted.editable = true;
    deleted.is_deleted = true;
    let deleted_items = context_menu_items(&deleted);
    let reset = deleted_items
        .iter()
        .find(|item| matches!(item.action, ContextAction::ResetCell { .. }))
        .expect("已删行要摆「重置单元格」");
    assert!(!reset.disabled, "已删行的「重置单元格」可点（它正是恢复通道）");

    // 记录视图（只读呈现）：编辑段整段不摆，自然也没有重置
    let mut record = target("orders", "name", 1);
    record.editable = true;
    record.record_mode = true;
    assert!(
        !context_menu_items(&record)
            .iter()
            .any(|item| matches!(item.action, ContextAction::ResetCell { .. })),
        "记录视图不摆「重置单元格」"
    );
}

/// 【第 64 批】行级三钮可用态的真值表（DBeaver `PROP_EDITABLE` 的 `add` / `copy` / `delete`
/// 三个分支，`ResultSetPropertyTester.java:137-158`）。与 `can_edit_cell` 的分工：
/// 那条管「编辑某一格」，这条管「行级增删复制」。
#[test]
fn row_action_state_follows_the_dbeaver_branches() {
    // 只读结果集：三钮全灰
    let read_only = row_action_state(false, true, false);
    assert!(!read_only.add && !read_only.copy && !read_only.delete);

    // 可编辑但没有当前行：只有「新增」可用（DBeaver 的 "add" 不要求 currentRow）
    let empty = row_action_state(true, false, false);
    assert!(empty.add);
    assert!(!empty.copy, "「复制」要 currentRow");
    assert!(!empty.delete, "「删除」要 currentRow");

    // 可编辑 + 有当前行 + 行未删：三钮全可用
    let row = row_action_state(true, true, false);
    assert!(row.add && row.copy && row.delete);

    // 已删行：新增 / 复制仍可用，删除灰（再删是宿主侧空操作）
    let deleted = row_action_state(true, true, true);
    assert!(deleted.add && deleted.copy);
    assert!(!deleted.delete, "已标记删除的行不能再删");
}

/// 【第 69 批】行级键盘目标：把「1 基当前行折 0 基 + 行删除态 + 可用态」收成一个真值表。
/// 布局三钮与五个键位入口都读它（`ResultPanel::row_target_state`）。
#[test]
fn row_target_state_packs_row_and_actions_for_the_keyboard() {
    // 可编辑 + 有当前行 2（1 基）→ 0 基 1，三组都可用
    let target = row_target_state(true, Some(1), false);
    assert_eq!(target.row, Some(1));
    assert!(!target.row_deleted);
    assert!(target.actions.add && target.actions.copy && target.actions.delete);

    // 没有当前行：`row` 为空，「新增」仍可用（DBeaver `add` 不要求 currentRow）
    let target = row_target_state(true, None, false);
    assert_eq!(target.row, None);
    assert!(target.actions.add);
    assert!(!target.actions.copy && !target.actions.delete);

    // 已删行：`row_deleted` 为真，「删除」不可用
    let target = row_target_state(true, Some(0), true);
    assert!(target.row_deleted);
    assert!(!target.actions.delete);

    // 只读结果集：三组全不可用，行号照旧透出（文案要区分「没选中」与「只读」）
    let target = row_target_state(false, Some(0), false);
    assert_eq!(target.row, Some(0));
    assert!(!target.actions.add && !target.actions.copy && !target.actions.delete);
}

/// 【第 62 批】编辑门控的真值表：三条条件（可编辑 / 非记录视图 / 有当前属性）必须同时成立。
/// 这是 DBeaver `ResultSetPropertyTester.PROP_EDITABLE` 的投影，状态栏「修改」钮与右键
/// 「修改单元格」共用它（两条入口分别在上下的测试里断言）。
#[test]
fn can_edit_cell_requires_all_three_conditions() {
    assert!(!can_edit_cell(false, false, true), "只读结果集不能编辑");
    assert!(
        !can_edit_cell(true, true, true),
        "记录视图是实现不了 IResultSetEditor 的呈现（PlainTextPresentation.java:63）"
    );
    assert!(
        !can_edit_cell(true, false, false),
        "没有当前属性（未选中 / 停在行号槽）"
    );
    assert!(can_edit_cell(true, false, true), "三条都满足才可编辑");
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
    let ix = with
        .iter()
        .position(|item| matches!(item.action, ContextAction::InsightColumn { .. }))
        .expect("可洞察时应摆「洞察此列」");
    assert_eq!(with[ix].label, "洞察「name」这一列");
    assert_eq!(
        with[ix].action,
        ContextAction::InsightColumn {
            column: "name".to_string()
        }
    );
    assert!(with[ix].separator_before, "它自成一段，前面要有分隔线");
    // 【布局段 / 选中统计 / 筛选条】layout 段是右键最后一段：洞察项之后只剩呈现方式组
    // 4 项、三颗统计开关、筛选条显隐 1 项（顺序照 `fillLayoutMenu:3549-3597`）；
    // `ix` 自身也算在内
    assert_eq!(
        with.len() - ix,
        9,
        "洞察项起算应只剩它自己 + 呈现方式组 4 项 + 三颗统计 + 筛选条显隐：{:?}",
        with[ix..].iter().map(|i| i.label.as_str()).collect::<Vec<_>>()
    );
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
    assert_eq!(
        grid.row_json_text(0),
        "{\n  \"id\": \"2\",\n  \"name\": null\n}"
    );
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

    grid.set_data(vec!["a".to_string()], vec![vec!["1".to_string()]]);
    assert!(!grid.is_frozen(1), "换了一份结果：冻结不跟过来");
}

/// 【B15】筛选只改视图行序：命中那些行还在原位，行号跟的是视图
#[test]
fn filtering_keeps_matching_rows_in_place() {
    let mut grid = grid_with(&[&["1", "orders"], &["2", "users"], &["3", "orders_archive"]]);
    grid.set_filter("orders");
    assert_eq!(grid.visible_row_count(), 2, "命中两行");
    assert_eq!(
        grid.data_row_count(),
        3,
        "数据一行没动（清掉筛选就能原样回来）"
    );
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
        assert_eq!(
            style.color_role,
            ColorRolePref::Inherit,
            "{kind:?} 默认不染色"
        );
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
    assert_eq!(
        format_cell_text(ColumnKind::Boolean, "1", &prefs),
        "1",
        "未知形态保持原文"
    );
    // 数字千分位（整数值分组）
    assert_eq!(
        format_cell_text(ColumnKind::Integer, "1234567", &prefs),
        "1,234,567"
    );
    // 非数字 / 不可解析保持原文
    assert_eq!(
        format_cell_text(ColumnKind::Text, "1234567", &prefs),
        "1234567"
    );
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
    assert_eq!(
        format_cell_text(ColumnKind::Decimal, "1234.5", &fixed),
        "1234.50"
    );
    assert_eq!(
        format_cell_text(ColumnKind::Float, "123456.789", &sci),
        "1.23456789e5"
    );
    // 解析失败回退原文
    assert_eq!(
        format_cell_text(ColumnKind::Decimal, "12.3.x", &fixed),
        "12.3.x"
    );
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

/// 【结果内查找 2026-11-18】反向查找与命中计数：口径必须与 `find_next` 完全对称
/// （大小写不敏感子串、只看看得见的行、`#` 槽不算、被删行不算）
#[test]
fn finding_backwards_and_counting_matches_mirror_forward() {
    let mut grid = grid_with(&[&["1", "orders"], &["2", "users"], &["3", "orders_archive"]]);
    assert_eq!(
        grid.count_find_matches("orders", FindCriteria::default()),
        2,
        "第三个格 `orders_archive` 也命中（子串口径），`#` 槽不参与"
    );
    assert_eq!(grid.count_find_matches("   ", FindCriteria::default()), 0, "空词一个也不算");

    // 反向：从末尾往回走，最后绕回最后一条
    assert_eq!(
        grid.find_prev("orders", FindCriteria::default(), None).map(|hit| (hit.row, hit.col)),
        Some((2, 2)),
        "没给起点时反向从末尾开始"
    );
    assert_eq!(
        grid
            .find_prev("orders", FindCriteria::default(), Some((2, 2)))
            .map(|hit| (hit.row, hit.col)),
        Some((0, 2)),
        "从上一处往前：中间行不命中，回到第 0 行"
    );
    assert_eq!(
        grid
            .find_prev("orders", FindCriteria::default(), Some((0, 2)))
            .map(|hit| (hit.row, hit.col)),
        Some((2, 2)),
        "到头绕回末尾"
    );
    assert_eq!(grid.find_prev("nope", FindCriteria::default(), None), None);

    // 筛选后只数看得见的行：`orders` 那两行里只剩一行还能看见
    grid.set_filter("users");
    assert_eq!(grid.count_find_matches("orders", FindCriteria::default()), 0, "看不见的行不计命中");
    assert_eq!(grid.count_find_matches("users", FindCriteria::default()), 1);
}

/// 结果内查找：只看**数据列**（`#` 槽不算）、按视图行序、找完一圈回到开头
#[test]
fn finding_in_the_result_walks_the_visible_rows_and_wraps() {
    let mut grid = grid_with(&[&["1", "orders"], &["2", "users"], &["3", "orders_archive"]]);
    assert_eq!(
        grid.find_next("orders", FindCriteria::default(), None),
        Some(ResultMatch {
            row: 0,
            col: 2,
            range: Some(0..6),
        }),
        "第一处命中的是**数据列**（行号槽不是数据）"
    );
    assert_eq!(
        grid.find_next("orders", FindCriteria::default(), Some((0, 2))).map(|hit| hit.row),
        Some(2),
        "从上一处继续：中间那行的第二列（users）不命中，继续往后找"
    );
    assert_eq!(
        grid.find_next("orders", FindCriteria::default(), Some((2, 2))).map(|hit| hit.row),
        Some(0),
        "找不到就绕回开头（表是圆的）"
    );

    // 行号槽里的数字不算命中（`#` 显示的是视图行号，不是数据）
    assert_eq!(
        grid.find_next("3", FindCriteria::default(), None),
        Some(ResultMatch {
            row: 2,
            col: 1,
            range: Some(0..1),
        })
    );
    assert_eq!(grid.find_next("nope", FindCriteria::default(), None), None);
    assert_eq!(grid.find_next("   ", FindCriteria::default(), None), None, "空词不找任何东西");

    // 含非 ASCII 的格：命中但不给格内区间（宿主退化成整格高亮）
    let chinese = grid_with(&[&["1", "订单_2026"]]);
    assert_eq!(
        chinese.find_next("2026", FindCriteria::default(), None),
        Some(ResultMatch {
            row: 0,
            col: 2,
            range: None,
        })
    );

    // 筛选之后只找**看得见的行**（找到看不见的行等于没找到）
    grid.set_filter("users");
    assert_eq!(grid.find_next("orders", FindCriteria::default(), None), None);
    assert_eq!(grid.find_next("users", FindCriteria::default(), None).map(|hit| hit.row), Some(0));
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
        assert_eq!(
            grid.column(1, cx).width,
            gpui_kit::px(240.),
            "id 列回填用户宽度"
        );
        assert_eq!(
            grid.column(2, cx).width,
            gpui_kit::px(96.),
            "name 列回填用户宽度"
        );
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
fn status_segments_report_total_and_selection() {
    let idle = ResultStatus {
        total_rows: 1_204,
        selected_row: None,
        selected_cell: None,
        truncated_hint: None,
        has_more: false,
        // 【选中统计】没选中时三项计数为 0（`ResultSetStatListener.java:80-90` 同口径）
        selected_rows: 0,
        selected_columns: 0,
        selected_cells: 0,
    };
    assert_eq!(
        status_segments(&idle),
        ("共 1,204 行".to_string(), "未选中行".to_string())
    );

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
        selected_rows: 0,
        selected_columns: 0,
        selected_cells: 0,
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
        selected_rows: 0,
        selected_columns: 0,
        selected_cells: 0,
    };
    assert_eq!(fetch_more_hint(&done), None);
    assert_eq!(truncation_notice(&done), None);
    assert!(!window_fetch_active(&done));

    // 分段中：「还有更多」，不出截断警告
    let partial = ResultStatus {
        has_more: true,
        ..done.clone()
    };
    assert_eq!(
        fetch_more_hint(&partial),
        Some("还有更多 · 滚动到底继续加载")
    );
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
/// 写操作段内部按 `editCmds` 再分两截（变更集动作 ‖ 行级动作，`plugin.xml:1407-1466`），
/// 翻页段四颗行导航（首页 · 上页 · 下页 · 末页）与 `navCmds` 同序（`plugin.xml:1470-1512`）；
/// 取下一段 / 取全部（`fetch.page` / `fetch.all`）是面板注入的真钮，不进占位清单
#[test]
fn status_action_placeholders_follow_dbeaver_group_order() {
    let slots = status_action_placeholders();
    assert_eq!(
        slots.len(),
        13,
        "占位钮共 13 个（取下一段 / 取全部真钮另由 controls.more / fetch_all 注入）"
    );

    // 写操作段 · 变更集动作：提交 · 回滚 · 撤销 · 重做
    let write_change: Vec<IconName> = slots[0..4].iter().map(|(icon, _)| *icon).collect();
    assert_eq!(
        write_change,
        vec![
            IconName::Save,
            IconName::X,
            IconName::Undo2,
            IconName::Redo2,
        ]
    );
    // 写操作段 · 行级动作：修改 · 新增行 · 复制行 · 删除行（DBeaver 在这两截之间有
    // `rowCmds.separator`，本仓同样插竖分隔）
    let write_row: Vec<IconName> = slots[4..FIRST_PAGER_ACTION]
        .iter()
        .map(|(icon, _)| *icon)
        .collect();
    assert_eq!(
        write_row,
        vec![
            IconName::Pencil,
            IconName::Plus,
            IconName::CopyPlus,
            IconName::Trash,
        ]
    );

    // 翻页段：首页 · 上页 · 下页 · 末页（下页本轮补上，此前只有取下一段真钮）
    let pager: Vec<IconName> = slots[FIRST_PAGER_ACTION..FIRST_SYSTEM_ACTION]
        .iter()
        .map(|(icon, _)| *icon)
        .collect();
    assert_eq!(
        pager,
        vec![
            IconName::SkipBack,
            IconName::ChevronLeft,
            IconName::ChevronRight,
            IconName::SkipForward
        ]
    );

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
        analyze: false,
        persist: false,
        table: None,
        column_filter_active: false,
        column_values: Vec::new(),
        sort_active: false,
        row_ref: RowRef::default(),
        editable: true,
        is_deleted: false,
        cell_changed: false,
        selection_stat_rows: false,
        selection_stat_columns: false,
        selection_stat_cells: false,
        // 列头菜单用例不摆布局段（呈现方式组只在单元格右键），字段填出厂态即可
        presentation: Presentation::Grid,
        record_mode: false,
        show_filter_panel: true,
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
    // 「复制列名」只属于列浏览器清单（网格右键没有多选集合这个概念）
    assert!(!labels.contains(&"复制列名"));
    // 没隐藏列就不摆「显示全部列」
    assert!(!labels.contains(&"显示全部列"));
}

/// 【DBeaver 对齐】列头菜单尾部筛选段四项（`ResultSetViewer.java:3926-3932`）：
/// 一条 Separator 之后连摆「保存过滤器设置 / 重置过滤器设置 / 删除所有过滤 / 排序 /
/// 编辑筛选设置」。DBeaver 只在容器是 `DBSEntity` 时摆前两项（`:3927-3930`）；本仓
/// 存档键是结果 SQL（§9.26.4 第 1 条），没有「非实体容器」这一层，四项恒摆。
#[test]
fn column_menu_ends_with_the_four_filter_commands() {
    let target = ContextTarget {
        value: String::new(),
        column_name: "id".to_string(),
        column: 0,
        row: 0,
        rows: 12,
        frozen: false,
        hidden_columns: 0,
        insight: false,
        analyze: false,
        persist: false,
        // 表名猜不到（多表 join / 计算列）：四项仍应恒摆
        table: None,
        column_filter_active: false,
        column_values: Vec::new(),
        sort_active: false,
        row_ref: RowRef::default(),
        editable: true,
        is_deleted: false,
        cell_changed: false,
        selection_stat_rows: false,
        selection_stat_columns: false,
        selection_stat_cells: false,
        // 列头菜单用例不摆布局段（呈现方式组只在单元格右键），字段填出厂态即可
        presentation: Presentation::Grid,
        record_mode: false,
        show_filter_panel: true,
    };
    let items = column_menu_items(&target);
    let tail_start = items.len() - 4;
    let tail: Vec<&str> = items[tail_start..]
        .iter()
        .map(|item| item.label.as_str())
        .collect();
    assert_eq!(
        tail,
        vec![
            "保存过滤器设置",
            "重置过滤器设置",
            "删除所有过滤 / 排序",
            "编辑筛选设置…",
        ],
        "尾部四项的顺序与文案（DBeaver zh bundle:84 / :78 / :86）"
    );
    assert_eq!(items[tail_start].action, ContextAction::SaveFilterSetting);
    assert_eq!(
        items[tail_start + 1].action,
        ContextAction::ResetFilterSetting
    );
    assert_eq!(items[tail_start + 2].action, ContextAction::ClearAllFilters);
    assert_eq!(
        items[tail_start + 3].action,
        ContextAction::OpenFilterSettings
    );
    // 只有第一项前有分隔线：DBeaver 是一条 Separator 之后四连摆（`:3926-3932`）
    assert!(items[tail_start].separator_before, "四项前应有分隔线");
    assert!(
        items[tail_start + 1..]
            .iter()
            .all(|item| !item.separator_before),
        "四项内部不再插分隔线"
    );
}

/// 列浏览器清单菜单 = 列头菜单的本地动作子集：复制 / 冻结 / 隐藏 / 列统计；
/// 排序下发 / 值筛选 / 洞察 / 查看完整值 / 复制整行不进清单入口（前者依赖宿主
/// 钩子，后者属于格 / 行交互，见拍板口径）。**「复制列名」只在清单入口**——
/// 它拷的是清单的多选集合，网格右键没有这个概念。
#[test]
fn column_browser_menu_is_local_column_subset() {
    let target = target("x", "id", 1);
    let items = column_browser_menu_items(&target);
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
    // 【DBeaver 对齐】`meta_data_panel_action_copy_column_text` = "Copy Column Names"
    assert_eq!(
        items[0].label, "复制列名",
        "清单右键第一项是「复制列名」（DBeaver fillCustomActions 的形状）"
    );
    assert_eq!(
        items[0].action,
        ContextAction::CopyColumnNames,
        "这一项的动作是纯 Clipboard 语义，不依赖宿主钩子"
    );
    assert!(labels.contains(&"复制此列（2 个取值）"));
    assert!(labels.contains(&"冻结「id」"));
    assert!(labels.contains(&"隐藏「id」"));
    assert!(labels.contains(&"列统计「id」"));
    // 不摆排序下发 / 值筛选 / 洞察 / 值与行动作
    assert!(
        !labels
            .iter()
            .any(|l| l.contains("升序") || l.contains("降序"))
    );
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
fn column_browser_menu_shows_conditional_items() {    let mut target = target("", "name", 2);
    target.frozen = true;
    target.hidden_columns = 2;
    target.sort_active = true;
    let items = column_browser_menu_items(&target);
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
    assert!(labels.contains(&"取消冻结此列"));
    assert!(labels.contains(&"显示全部列"));
    assert!(labels.contains(&"取消排序「name」"));
}

/// 【结果集二次分析】右键菜单的整表入口：`analyze` 才摆，文案带行数；
/// 与「洞察此列」同组（insight 也在时不自带分隔线）
#[test]
fn context_menu_shows_analyze_result_set_only_when_available() {
    let mut t = target("1", "id", 1);
    let labels_of = |t: &ContextTarget| -> Vec<String> {
        context_menu_items(t)
            .iter()
            .map(|i| i.label.clone())
            .collect()
    };

    // 没有钩子（`analyze = false`）→ 不摆（能力没有就不给入口）
    assert!(!labels_of(&t).iter().any(|l| l.contains("分析这份结果集")));

    t.analyze = true;
    let items = context_menu_items(&t);
    let analyze_item = items
        .iter()
        .find(|i| i.label.contains("分析这份结果集"))
        .expect("有入口时要摆这一项");
    assert_eq!(analyze_item.label, "分析这份结果集…（2 行）");
    assert!(matches!(
        analyze_item.action,
        ContextAction::AnalyzeResultSet
    ));
    assert!(analyze_item.separator_before, "insight 不在时自带分隔线");

    // 「洞察此列」也在时并入同一组（不再插分隔线）
    t.insight = true;
    let items = context_menu_items(&t);
    let analyze_item = items
        .iter()
        .find(|i| i.label.contains("分析这份结果集"))
        .expect("两项都在");
    assert!(!analyze_item.separator_before);

    // 【结果集持久化】同一份结果的第二个去向：`persist` 才摆，文案带行数
    assert!(!items.iter().any(|i| i.label.contains("升级为资产")));
    t.persist = true;
    let items = context_menu_items(&t);
    let persist_item = items
        .iter()
        .find(|i| i.label.contains("升级为资产"))
        .expect("有入口时要摆这一项");
    assert_eq!(persist_item.label, "升级为资产（2 行）");
    assert!(matches!(
        persist_item.action,
        ContextAction::PersistResultSet
    ));

    // 列头 / 清单菜单只做列级动作：整表入口不出现在这两处
    let head = column_menu_items(&t);
    assert!(!head.iter().any(|i| i.label.contains("分析这份结果集")));
    assert!(!head.iter().any(|i| i.label.contains("升级为资产")));
    let browser = column_browser_menu_items(&t);
    assert!(!browser.iter().any(|i| i.label.contains("分析这份结果集")));
    assert!(!browser.iter().any(|i| i.label.contains("升级为资产")));
}

/// 清单菜单动作归属：本地动作直接映射到对应 `ContextAction`，列号不串位。
#[test]
fn column_browser_menu_actions_map_to_local_column_actions() {
    let target = target("", "name", 3);
    let items = column_browser_menu_items(&target);
    let by_label: std::collections::HashMap<&str, &ContextAction> = items
        .iter()
        .map(|i| (i.label.as_str(), &i.action))
        .collect();
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
        analyze: false,
        persist: false,
        table: Some("t_user".to_string()),
        column_filter_active: false,
        column_values: Vec::new(),
        sort_active: false,
        row_ref: RowRef::default(),
        editable: true,
        is_deleted: false,
        cell_changed: false,
        selection_stat_rows: false,
        selection_stat_columns: false,
        selection_stat_cells: false,
        // 列头菜单用例不摆布局段（呈现方式组只在单元格右键），字段填出厂态即可
        presentation: Presentation::Grid,
        record_mode: false,
        show_filter_panel: true,
    };
    let items = column_menu_items(&target);
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
    assert!(labels.contains(&"显示全部列"));
    assert!(labels.contains(&"洞察「name」这一列"));
    assert!(labels.contains(&"取消冻结此列"));
    // 表名只喂给 INSERT（右键项）；列头菜单不含它
    assert!(!labels.iter().any(|l| l.contains("INSERT")));
}

/// 【DBeaver 对齐】列头菜单的「复制列名」（`CMD_COPY_COLUMN_NAMES`，
/// `ResultSetViewer.java:3151-3161`）：文案带列名、排在复制段首位（在「复制此列」之前）；
/// 单元格右键不摆——右键的「复制此列」复制的是**取值**，列名在那里没有落点。
#[test]
fn column_menu_offers_copying_the_column_name() {
    let target = target("", "status", 1);
    let items = column_menu_items(&target);
    let copy_name = items
        .iter()
        .position(|i| matches!(i.action, ContextAction::CopyColumnName))
        .expect("列头菜单要摆「复制列名」");
    assert_eq!(items[copy_name].label, "复制列名「status」");
    assert!(!items[copy_name].separator_before);

    // 位置：复制段首位（拿列名拼 SQL 比拷整列取值更靠前）
    let copy_column = items
        .iter()
        .position(|i| matches!(i.action, ContextAction::CopyColumn(_)))
        .expect("列头菜单一直有「复制此列」");
    assert!(
        copy_name < copy_column,
        "「复制列名」要排在「复制此列」之前"
    );

    // 单元格右键不摆（DBeaver 的 DATA 落点只有 EDIT_COPY，没有复制列名）
    let ctx = context_menu_items(&target);
    assert!(!ctx
        .iter()
        .any(|i| matches!(i.action, ContextAction::CopyColumnName)));
}

/// 【DBeaver 对齐】行号槽右键只有「复制行号」（`ROW_HEADER` → `CMD_COPY_ROW_NAMES`，
/// `ResultSetViewer.java:3162-3164`；handler `ResultSetHandlerMain.java:377-396` 取
/// `getVisualNumber() + 1`）：文案带要拷的号、载荷就是那串显示行号；
/// 行号不是数据，数据列的动作（筛选 / 排序 / 冻结 / 统计）在这里一个都不摆。
#[test]
fn row_number_menu_offers_copying_the_visible_row_number() {
    let items = row_number_menu_items("3");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].label, "复制行号「3」");
    assert!(!items[0].separator_before);
    assert_eq!(
        items[0].action,
        ContextAction::CopyRowNumber("3".to_string())
    );
    // 单元格右键的纯函数不产出这一项（它只属于行号落点）
    let ctx = context_menu_items(&target("v", "status", 1));
    assert!(!ctx
        .iter()
        .any(|i| matches!(i.action, ContextAction::CopyRowNumber(_))));
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
        analyze: false,
        persist: false,
        table: None,
        column_filter_active: false,
        column_values: vec![
            "paid".to_string(),
            "NULL".to_string(),
            "refunded".to_string(),
        ],
        sort_active: false,
        row_ref: RowRef::default(),
        editable: true,
        is_deleted: false,
        cell_changed: false,
        selection_stat_rows: false,
        selection_stat_columns: false,
        selection_stat_cells: false,
        // 列头菜单用例不摆布局段（呈现方式组只在单元格右键），字段填出厂态即可
        presentation: Presentation::Grid,
        record_mode: false,
        show_filter_panel: true,
    };
    let items = column_menu_items(&target);
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
    assert!(labels.contains(&"按值筛选「paid」"));
    assert!(labels.contains(&"按值筛选「NULL」"));
    assert!(labels.contains(&"按值筛选「refunded」"));
    // 值列表第一项带分隔线（与上方排序段隔开）
    let first_value = items.iter().find(|i| i.label.contains("按值筛选")).unwrap();
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
    assert!(
        !column_menu_items(&empty)
            .iter()
            .any(|i| i.label.contains("按值筛选"))
    );
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
    let rows = vec![
        vec!["NULL".to_string()],
        vec!["NULL".to_string()],
        vec!["ok".to_string()],
    ];
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
    assert_eq!(
        grid.visible_rows(),
        vec![vec!["3".to_string(), "alice".to_string()]]
    );

    // 清除列筛选 → 全局筛选单独生效
    grid.set_column_filter(None);
    assert_eq!(
        grid.visible_rows(),
        vec![vec!["3".to_string(), "alice".to_string()]]
    );
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
/// 【列序】显示序是视图列序的**权威**：程序化移动 / 按列名排序 / 重置都要
/// 同时改写 `display_order` 与折算缓存，隐藏列仍照旧被跳过。
///
/// 回应对齐 DBeaver `FilterSettingsDialog` 工具条的移动四钮 + `SORT`
/// （`FilterSettingsDialog.java:278-302`）——那边改的是约束的 `visualPosition`。
#[test]
fn display_order_drives_the_view_column_mapping() {
    let mut grid = ResultGridDelegate::default();
    grid.set_data(
        vec![
            "id".to_string(),
            "name".to_string(),
            "amount".to_string(),
        ],
        vec![vec![
            "1".to_string(),
            "alice".to_string(),
            "12".to_string(),
        ]],
    );

    // 默认：显示序 = 数据序
    assert_eq!(grid.data_column(2), 2, "默认可见列 2 = 数据列 2");
    assert_eq!(grid.view_column(0), Some(0));

    // 把数据列 2（amount）移到最前：视图 0 号位应指向它
    grid.move_display_column(2, 0);
    assert_eq!(
        grid.display_order(),
        vec!["amount".to_string(), "id".to_string(), "name".to_string()]
    );
    assert_eq!(grid.data_column(0), 2, "移顶后席位 0 = amount（数据列 2）");
    assert_eq!(grid.data_column(1), 0, "席位 1 = id");
    assert_eq!(grid.view_column(2), Some(0), "amount 的可见席位是 0");

    // 上移一列（amount 已在首位 → 无变化，调用方按 enabled 挡住）
    grid.move_display_column(2, 0);
    assert_eq!(grid.data_column(0), 2, "已在首位，目标相同即不动");

    // 下移一列：amount 回到席位 1
    grid.move_display_column(2, 1);
    assert_eq!(grid.data_column(0), 0, "席位 0 回到 id");
    assert_eq!(grid.data_column(1), 2, "席位 1 = amount");

    // 移底：amount 到末位
    grid.move_display_column(2, 2);
    assert_eq!(
        grid.display_order(),
        vec!["id".to_string(), "name".to_string(), "amount".to_string()],
        "移底 = 回到数据序（本份结果只有三列）"
    );

    // 隐藏中间列后：移动仍按**可见席位**算，隐藏列不参与也不丢
    grid.hide_column(1);
    grid.move_display_column(2, 0);
    assert_eq!(grid.data_column(0), 2, "隐藏 name 后 amount 仍是席位 0");
    assert_eq!(grid.view_column(1), None, "隐藏列没有可见席位");
    assert_eq!(
        grid.display_order().len(),
        3,
        "display_order 含全部列（隐藏列不从这里删）"
    );

    // 按列名排序（A→Z）：amount / id / name
    grid.sort_display_order_by_name();
    assert_eq!(
        grid.display_order(),
        vec!["amount".to_string(), "id".to_string(), "name".to_string()],
        "按列名 A→Z 重排"
    );
    assert_eq!(grid.data_column(0), 2, "排序后席位 0 = amount");
    assert_eq!(grid.data_column(1), 0, "隐藏 name 后席位 1 = id");

    // 重置列布局：列序回数据序 + 显示全部 + 取消冻结
    grid.toggle_freeze(0);
    grid.reset_column_layout();
    assert_eq!(
        grid.display_order(),
        vec!["id".to_string(), "name".to_string(), "amount".to_string()],
        "重置后列序回数据序"
    );
    assert!(grid.frozen_column_snapshot().is_empty(), "冻结被清空");
    assert_eq!(grid.data_column(0), 0, "席位 0 回到 id");
    assert_eq!(grid.data_column(2), 2, "末位回到 amount");
}

#[test]
fn hidden_columns_are_skipped_in_view_but_kept_in_data() {
    let mut grid = ResultGridDelegate::default();
    grid.set_data(
        vec!["id".to_string(), "name".to_string(), "blob".to_string()],
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

/// 【P2-3】批量显隐：整批隐藏 / 整批显示；整批隐藏到"全藏"时仍保留至少 1 列
#[test]
fn set_columns_hidden_keeps_one_visible_column() {
    let mut grid = ResultGridDelegate::default();
    grid.set_data(
        vec!["a".to_string(), "b".to_string(), "c".to_string()],
        vec![vec!["1".to_string(), "2".to_string(), "3".to_string()]],
    );

    // 整批隐藏 0 / 2：两列都藏，剩 1 列可见
    grid.set_columns_hidden(&[0, 2], true);
    assert!(grid.is_hidden_column(0) && grid.is_hidden_column(2));
    assert_eq!(grid.visible_column_count(), 1);

    // 再把最后一列也藏进去 —— 底线生效，可见列数不会掉到 0
    grid.set_columns_hidden(&[1], true);
    assert_eq!(grid.visible_column_count(), 1, "至少保留 1 列可见");

    // 整批显示：全部放回
    grid.set_columns_hidden(&[0, 1, 2], false);
    assert_eq!(grid.visible_column_count(), 3);
    assert_eq!(grid.hidden_count(), 0);
}

/// 【列布局落盘】隐藏 / 冻结快照按**列名**往返：新结果换了列序，按名字回填仍钉对列；
/// 列名对不上静默跳过；隐藏仍守「至少 1 列可见」底线
#[test]
fn column_layout_snapshots_round_trip_by_name() {
    let mut grid = ResultGridDelegate::default();
    grid.set_data(
        vec!["id".to_string(), "name".to_string(), "blob".to_string()],
        vec![vec!["1".to_string(), "alice".to_string(), "aabb".to_string()]],
    );
    grid.hide_column(1);
    grid.toggle_freeze(2);

    // 快照是列名，不是下标（下标会随列序重排而失效）
    assert_eq!(grid.hidden_column_snapshot(), vec!["name".to_string()]);
    assert_eq!(grid.frozen_column_snapshot(), vec!["blob".to_string()]);

    // 新结果把列序倒过来（模拟持久化列序重排）：set_data 清空两张清单
    grid.set_data(
        vec!["blob".to_string(), "name".to_string(), "id".to_string()],
        vec![vec!["aabb".to_string(), "alice".to_string(), "1".to_string()]],
    );
    assert_eq!(grid.visible_column_count(), 3, "set_data 后回到全部可见");
    assert_eq!(grid.hidden_count(), 0);

    // 按名字回填：隐藏 name（新下标 1）与冻结 blob（新下标 0）
    grid.apply_hidden_columns(&["name".to_string(), "missing".to_string()]);
    grid.apply_frozen_columns(&[
        "blob".to_string(),
        "blob".to_string(),
        "missing".to_string(),
    ]);
    assert!(grid.is_hidden_column(1), "按列名钉到新列序的 name");
    assert_eq!(grid.visible_column_count(), 2);
    assert!(grid.is_frozen(0), "按列名钉到新列序的 blob");
    assert_eq!(grid.frozen_column_snapshot(), vec!["blob".to_string()]);
}

/// 【列布局落盘】回填整列全隐藏的旧快照：仍守「至少 1 列可见」（与交互路径同一份底线）
#[test]
fn apply_hidden_columns_still_keeps_one_visible_column() {
    let mut grid = ResultGridDelegate::default();
    grid.set_data(
        vec!["a".to_string(), "b".to_string()],
        vec![vec!["1".to_string(), "2".to_string()]],
    );
    grid.apply_hidden_columns(&["a".to_string(), "b".to_string()]);
    assert_eq!(grid.visible_column_count(), 1, "至少保留 1 列可见");
    assert!(grid.is_hidden_column(0) || grid.is_hidden_column(1));
}

/// 【侧栏】单列取消隐藏：`show_column` 只放回这一列，其它隐藏列原样保留
/// （与「显示全部」整批清空的粒度区别开——左栏的眼睛是单列粒度）
#[test]
fn show_column_restores_only_that_column() {
    let mut grid = ResultGridDelegate::default();
    grid.set_data(
        vec!["a".to_string(), "b".to_string(), "c".to_string()],
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
        vec!["id".to_string(), "name".to_string(), "blob".to_string()],
        vec![
            vec!["1".to_string(), "alice".to_string(), "aabb".to_string()],
            vec!["2".to_string(), "bob".to_string(), "ccdd".to_string()],
        ],
    );
    let text = grid.text_presentation();
    assert_eq!(
        text, "id\tname\tblob\n1\talice\taabb\n2\tbob\tccdd",
        "全列 TSV（列头 + 行）"
    );
    grid.hide_column(2);
    assert_eq!(
        grid.text_presentation(),
        "id\tname\n1\talice\n2\tbob",
        "隐藏列不参与文本"
    );
    grid.set_filter("bob");
    assert_eq!(
        grid.text_presentation(),
        "id\tname\n2\tbob",
        "筛选只留命中的行"
    );
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
        vec![
            ("x".to_string(), 2),
            ("NULL".to_string(), 1),
            ("y".to_string(), 1)
        ],
        "NULL 独立成组"
    );
    assert!(
        ResultGridDelegate::group_by_count(&rows, 9).is_empty(),
        "越界列空组"
    );
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
    assert_eq!(
        column_stats_from_values(&values),
        column_stats(&rows, 0),
        "统计列级内核与矩阵版一致"
    );

    let name_values: Vec<String> = rows.iter().map(|row| row[1].clone()).collect();
    assert_eq!(
        group_by_values(&name_values),
        ResultGridDelegate::group_by_count(&rows, 1),
        "分组列级内核与矩阵版一致"
    );
}

/// 【P2-2】delegate 列级访问器跟随筛选 / 排序后的**视图行序**，结果与矩阵
/// `visible_rows()` 现算一致——消除整行物化后口径不能变。
#[test]
fn delegate_column_accessors_follow_view_rows_like_matrix_version() {
    let mut grid = grid_with(&[&["10", "a"], &["20", "b"], &["NULL", "a"], &["30", "b"]]);

    // 全量视图：统计 / 分组与矩阵版逐项相等
    let rows = grid.visible_rows();
    assert_eq!(grid.column_stats_for(0), column_stats(&rows, 0));
    assert_eq!(
        grid.group_by_for(1),
        ResultGridDelegate::group_by_count(&rows, 1)
    );
    assert_eq!(
        grid.group_by_for(1),
        vec![("a".to_string(), 2), ("b".to_string(), 2)],
        "首现顺序 + 计数"
    );

    // 筛选后只剩 name 含 a 的两行：访问器同样只统计视图行
    grid.set_filter("a");
    let filtered = grid.visible_rows();
    assert_eq!(
        grid.column_stats_for(0),
        column_stats(&filtered, 0),
        "统计跟随筛选行序"
    );
    assert_eq!(
        grid.group_by_for(1),
        ResultGridDelegate::group_by_count(&filtered, 1),
        "分组跟随筛选行序"
    );
    assert_eq!(grid.column_stats_for(0).count, 1, "10 那行非空");
    assert_eq!(grid.column_stats_for(0).nulls, 1, "NULL 那行被筛进来");
}

/// 【P2-3】展示行：数值给全，纯文本列只给计数两行
#[test]
fn stats_lines_are_key_value_pairs() {
    let stats = column_stats(&[vec!["1.5".to_string(), "NULL".to_string()]], 0);
    assert_eq!(
        stats_lines(&stats),
        [
            "值 1 个",
            "NULL 0 个",
            "最小 1.5",
            "最大 1.5",
            "均值 1.5",
            "合计 1.5"
        ]
    );
    assert_eq!(fmt_stat_num(1.0), "1", "去尾零");
    assert_eq!(fmt_stat_num(1.500000), "1.5");
}

/// 【R2】千分位格式化：整数每 3 位插逗号、小数去尾零（对齐 DBeaver Calc 的
/// `DOUBLE_FORMAT` = `###,###,###,###,###,##0.###`）
#[test]
fn fmt_grouped_inserts_thousand_separators() {
    assert_eq!(fmt_grouped(0.0), "0");
    assert_eq!(fmt_grouped(999.0), "999");
    assert_eq!(fmt_grouped(1000.0), "1,000");
    assert_eq!(fmt_grouped(1234.0), "1,234");
    assert_eq!(fmt_grouped(1234567.0), "1,234,567");
    assert_eq!(fmt_grouped(-1234.5), "-1,234.5");
    // 非有限值不分组（沿用原样文本）
    assert_eq!(fmt_grouped(f64::NAN), "NaN");
}

/// 【R2】字符串口径极值：排除 NULL 后按字典序取最小 / 最大（文本列也能给极值）
#[test]
fn text_minmax_ignores_null_and_orders_lexically() {
    let values = vec![
        "banana".to_string(),
        "NULL".to_string(),
        "apple".to_string(),
        "cherry".to_string(),
    ];
    let (min, max) = text_minmax(&values);
    assert_eq!(min.as_deref(), Some("apple"));
    assert_eq!(max.as_deref(), Some("cherry"));
    // 全 NULL / 空列 = 无极值（渲染层出 —）
    assert_eq!(text_minmax(&["NULL".to_string()]), (None, None));
    assert_eq!(text_minmax(&[] as &[String]), (None, None));
}

/// 【R2 计算页·去重计数】NULL 不计入；按值文本判等（`1` 与 `1.0` 算两个值，登记在册）
#[test]
fn distinct_count_skips_null_and_counts_text_values() {
    let values = vec![
        "a".to_string(),
        "NULL".to_string(),
        "b".to_string(),
        "a".to_string(),
        "1".to_string(),
        "1.0".to_string(),
    ];
    assert_eq!(distinct_count(&values), 4, "a / b / 1 / 1.0");
    assert_eq!(distinct_count(&["NULL".to_string()]), 0, "全 NULL = 0 个不同值");
    assert_eq!(distinct_count(&[] as &[String]), 0);
}

/// 【R2 计算页·中位数】只取可解析的数值样本：奇数取中间、偶数取中间两个的均值；
/// 没有数值样本 = None（渲染层出 —）
#[test]
fn median_of_uses_sorted_numeric_samples() {
    let odd = vec!["3".to_string(), "1".to_string(), "NULL".to_string(), "2".to_string()];
    assert_eq!(median_of(&odd), Some(2.0));
    let even = vec!["4".to_string(), "1".to_string(), "3".to_string(), "2".to_string()];
    assert_eq!(median_of(&even), Some(2.5));
    // 文本样本不参与；全是文本 = None
    assert_eq!(median_of(&["apple".to_string(), "zebra".to_string()]), None);
    assert_eq!(median_of(&[] as &[String]), None);
    // 单个数值样本：中位数就是它自己
    assert_eq!(median_of(&["7.5".to_string()]), Some(7.5));
}

/// 【R2 计算页·众数】出现最多的非空值；并列时取**首现更早**的那个（确定性口径）；
/// 全 NULL = None
#[test]
fn mode_of_picks_most_frequent_value_and_breaks_ties_by_first_occurrence() {
    let values = vec![
        "b".to_string(),
        "a".to_string(),
        "b".to_string(),
        "NULL".to_string(),
        "a".to_string(),
    ];
    assert_eq!(mode_of(&values).as_deref(), Some("b"), "b 与 a 都是 2 次，b 先出现");
    let clear_winner = vec!["x".to_string(), "y".to_string(), "y".to_string()];
    assert_eq!(mode_of(&clear_winner).as_deref(), Some("y"));
    assert_eq!(mode_of(&["NULL".to_string()]), None);
    assert_eq!(mode_of(&[] as &[String]), None);
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
        vec![vec![
            "1".to_string(),
            "alice".to_string(),
            "aabb".to_string(),
        ]],
    );
    grid.hide_column(1);
    grid.set_transposed(true, 0);
    assert_eq!(
        grid.transpose_columns(),
        vec![0, 2],
        "隐藏的 name 不参与转置"
    );
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
    assert!(grid.row_json_text(2).contains("zoe"), "行 JSON 纳入本地行");
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
    grid.set_local_snapshot(Some(local_snapshot(vec![String::new(), "zoe".to_string()])));
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

/// 【类型感知编辑】布尔列走双态按钮：定值即提交，且**同一条** `commit_edit` 通道——
/// 变更集钩子收到原值/新值、编辑态清空；非法值仍被校验挡住并保留编辑态（红框提示）。
#[test]
fn boolean_edit_commits_through_the_same_channel() {
    use crate::store::ColumnType;
    let mut grid = grid_with(&[&["1", "yes"]]);
    grid.set_editable(true);
    // 数据列 1（组件列 2，name）标成 boolean；列 0 不带类型
    grid.set_column_types(vec![None, Some(ColumnType::parse("boolean"))]);

    let seen = Rc::new(std::cell::RefCell::new(Vec::<(
        usize,
        usize,
        String,
        String,
    )>::new()));
    let seen_for_hook = seen.clone();
    grid.set_commit_edit_hook(Rc::new(move |row, col, orig, edited| {
        seen_for_hook.borrow_mut().push((row, col, orig, edited));
    }));

    assert!(grid.begin_edit(0, 2), "布尔数据列可进入编辑");
    assert!(grid.commit_edit_as("true"), "双态定值即提交");
    assert_eq!(
        seen.borrow().as_slice(),
        &[(0, 1, "yes".to_string(), "true".to_string())],
        "钩子收到 原值 yes → 新值 true，走的是变更集通道"
    );
    assert!(!grid.is_editing(0, 2), "提交后编辑态清空");

    // 非布尔值：双态按钮本身不会发，但外部调用也要挡住（校验是唯一闸口）
    assert!(grid.begin_edit(0, 2));
    assert!(!grid.commit_edit_as("maybe"), "非法值被校验拒绝");
    assert!(grid.is_editing(0, 2), "被拒后编辑态保留（红框提示）");
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
    assert!(grid.row_insert_text(1).is_empty(), "本地行不拼 INSERT");
    assert!(!grid.row_insert_text(0).is_empty(), "底层行照常拼 INSERT");
}

// ===== 【#28-B5】全链路排除本地行 / 被删行（排除矩阵收口） =====

/// 构造：底层 3 行，标记底层第 2 行（Data(1)，bob）删除，追加 1 个本地行（zoe）。
fn grid_with_deleted_and_local() -> ResultGridDelegate {
    let mut grid = grid_with(&[&["1", "alice"], &["2", "bob"], &["3", "carol"]]);
    let mut local = LocalRowSet::new();
    local.mark_deleted(1);
    let id = local.next_id();
    local.push_local(LocalRow::new(id, vec!["9".to_string(), "zoe".to_string()]));
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

    let none = ResultToolbar {
        rows: Some(2),
        ..Default::default()
    };
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
    Arc::get_mut(&mut fact)
        .expect("测试独占事实帧")
        .release_batches();
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
        vec![Arc::new(Float64Array::from(vec![
            Some(1.0),
            Some(2.0),
            Some(1.0),
        ]))],
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

/// 【R3 多列归一化】多列分组里数值列同样按原生值取键：`2.0` 与 `2` 不会被拆成两组，
/// 与单列 [`ResultGridDelegate::group_by_for`] 同口径；文本列仍按显示文本。
#[test]
fn group_by_columns_normalizes_numeric_keys_beside_text_columns() {
    let fact = fact_frame(
        &["price", "name"],
        vec![
            Arc::new(Float64Array::from(vec![
                Some(2.0),
                Some(2.0),
                Some(3.0),
            ])),
            Arc::new(StringArray::from(vec![Some("a"), Some("a"), Some("b")])),
        ],
    );
    let mut grid = ResultGridDelegate::empty("尚未执行");
    grid.set_data(
        vec!["price".to_string(), "name".to_string()],
        vec![
            vec!["2".to_string(), "a".to_string()],
            // 同一数值的另一种显示文本：纯文本口径下会被拆成两组
            vec!["2.0".to_string(), "a".to_string()],
            vec!["3".to_string(), "b".to_string()],
        ],
    );
    grid.set_fact(Some(fact));
    assert_eq!(
        grid.group_by_columns_for(&[0, 1]),
        vec![
            (vec!["2".to_string(), "a".to_string()], 2),
            (vec!["3".to_string(), "b".to_string()], 1),
        ],
        "数值列按原生值归一化，文本列按显示文本"
    );

    // 对照：同一份矩阵没有事实档时按文本等值分组，`2` / `2.0` 各自成组（老口径）
    let mut plain = ResultGridDelegate::empty("尚未执行");
    plain.set_data(
        vec!["price".to_string(), "name".to_string()],
        vec![
            vec!["2".to_string(), "a".to_string()],
            vec!["2.0".to_string(), "a".to_string()],
            vec!["3".to_string(), "b".to_string()],
        ],
    );
    assert_eq!(
        plain.group_by_columns_for(&[0, 1]).len(),
        3,
        "无事实档无法归一化：2 与 2.0 各自成组"
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

/// 【P2 摸底】`sync()` 全量重投影的成本探针（50k × 8，与文本视图基准同规格）。
///
/// `ResultPanel::sync()` 每次都把 `GridPayload.rows` 灌进 `set_data`（含视图行重建），
/// 编辑提交回执 / 脏格更新这些轻量状态变化也走同一条路。本探针量出三段成本：
/// 1. `reorder_by` **空序透传**（持久化列序为空 = 大多数会话；rows move 直通，应近零）；
/// 2. `reorder_by` **带持久化列序重排**（#296 列序持久化生效后每次 sync 的新增成本）；
/// 3. `set_data` **全量重灌**（sync 的主体成本；第二次灌入 = 编辑回执后的重投影）。
///
/// 观察口径：release 模式 `cargo test -p rds-result-panel --release bench_set_data -- --nocapture`；
/// 分段抓取默认 200 行时这些成本无感，只有 FetchAll 到几万行后才可能成为「编辑卡一下」
/// 的来源——数据超过百毫秒量级时再考虑投影代次短路（`GridPayload` 指纹跳过 `set_data`）。
#[test]
fn bench_set_data_resync_large_set() {
    let cols: Vec<String> = (0..8).map(|i| format!("col{}", i)).collect();
    let rows: Vec<Vec<String>> = (0..50_000)
        .map(|r| (0..8).map(|c| format!("v{}:{}", r, c)).collect())
        .collect();

    // 1) 空序透传（clone 在计时窗口外：只量 move 直通）
    let cols_a = cols.clone();
    let rows_a = rows.clone();
    let t0 = std::time::Instant::now();
    let _ = crate::panel::events::reorder_by(&[], cols_a, rows_a);
    let passthrough = t0.elapsed();

    // 2) 带持久化列序：倒序重排（每次 sync 对全部行逐格重排）
    let cols_b = cols.clone();
    let rows_b = rows.clone();
    let order: Vec<String> = cols.iter().rev().cloned().collect();
    let t1 = std::time::Instant::now();
    let _ = crate::panel::events::reorder_by(&order, cols_b, rows_b);
    let reordered = t1.elapsed();

    // 3) set_data 全量重灌：先灌一次（构造期），第二次计时（模拟编辑回执后的重投影）
    let mut grid = ResultGridDelegate::default();
    grid.set_data(cols.clone(), rows.clone());
    let cols_c = cols;
    let rows_c = rows;
    let t2 = std::time::Instant::now();
    grid.set_data(cols_c, rows_c);
    let resync = t2.elapsed();

    eprintln!(
        "BENCH sync projection 50k x 8: reorder passthrough={:?} reordered={:?} set_data resync={:?}",
        passthrough, reordered, resync
    );
}

/// 只为了拿到 `Window`（文本测量需要 `window.text_system()`）的空视图
struct BlankView;
impl gpui_kit::Render for BlankView {
    fn render(
        &mut self,
        _window: &mut gpui_kit::Window,
        _cx: &mut gpui_kit::Context<Self>,
    ) -> impl gpui_kit::IntoElement {
        gpui_kit::div()
    }
}

/// 【P1 列宽自适应】双击列头分隔线：行号槽拒绝（`None`）、宽度随内容增长、采样只覆盖前
/// [`FIT_SAMPLE_ROWS`] 个视图行
///
/// 断言一律用**相对比较**（宽列 > 窄列、结果落在某个数量级内），不写死具体像素：字体族、
/// 缩放、字形度量都随系统变，写死只会得到“在别人机器上必挂”的用例。
#[gpui_kit::test]
fn fit_column_follows_content_and_caps_the_sample(cx: &mut gpui_kit::TestAppContext) {
    use crate::grid::fit::FIT_SAMPLE_ROWS;
    use gpui_kit::px;

    cx.update(gpui_kit::init);
    let (_, mut vcx) = cx.add_window_view(|_window, _cx| BlankView);
    let fit = |grid: &ResultGridDelegate, col_ix: usize, vcx: &mut gpui_kit::VisualTestContext| {
        vcx.update(move |window, cx| crate::grid::fit::fit_column(grid, col_ix, window, cx))
    };

    // 1) 行号槽：不可自适应（双击它不该发生任何事）
    let plain = grid();
    assert_eq!(fit(&plain, 0, &mut vcx), None, "行号槽不该自适应");

    // 2) 内容越宽，量出来的列越宽（窄列 a / 宽列中文）
    let mut wide = ResultGridDelegate::empty("尚未执行");
    wide.set_data(
        vec!["a".to_string(), "b".to_string()],
        vec![
            vec!["a".to_string(), "这是一段明显更长的内容".to_string()],
            vec!["b".to_string(), "这是另一段明显更长的内容".to_string()],
        ],
    );
    let narrow_w = fit(&wide, 1, &mut vcx).expect("数据列应自适应");
    let wide_w = fit(&wide, 2, &mut vcx).expect("数据列应自适应");
    assert!(
        wide_w > narrow_w,
        "长内容列应更宽：narrow={narrow_w:?} wide={wide_w:?}"
    );
    // 至少包住单元格左右内边距（只算内容宽的实现会小于这个值）
    assert!(
        narrow_w >= px(16.),
        "列宽应含单元格内边距：{narrow_w:?}"
    );

    // 3) 采样上限：第 FIT_SAMPLE_ROWS+1 行的超长值不参与——否则宽度会到几千像素
    let mut many = ResultGridDelegate::empty("尚未执行");
    let mut rows: Vec<Vec<String>> = (0..FIT_SAMPLE_ROWS)
        .map(|_| vec!["x".to_string(), "a".to_string()])
        .collect();
    rows.push(vec!["y".to_string(), "W".repeat(400)]);
    many.set_data(vec!["a".to_string(), "b".to_string()], rows);
    let capped = fit(&many, 2, &mut vcx).expect("数据列应自适应");
    assert!(
        capped < px(100.),
        "第 {FIT_SAMPLE_ROWS} 行之后的超长值不该进采样：{capped:?}"
    );
}

/// 【排序图标槽】自适应宽度必须给组件的排序图标槽留出 20px
///
/// 组件在可排序列的表头右端放一个 20px 的图标槽（12px 图标 + 2px 内距 + 4px 右侧偏移，
/// 见 `ui::RESULT_FIT_HEADER_CHROME`）。列名足够长时表头块宽压过单元格块宽，量出的宽度
/// 就该**恰好等于「列名 + 20」**——漏算这 20px，双击自适应出来的列宽会把列名挤成省略号
/// （128px 列的表头格实测只有 108px，差的正是这 20px）。
#[gpui_kit::test]
fn autofit_reserves_the_component_sort_icon_slot(cx: &mut gpui_kit::TestAppContext) {
    use crate::ui::RESULT_FIT_HEADER_CHROME;
    use gpui_kit::px;

    cx.update(gpui_kit::init);
    let (_, vcx) = cx.add_window_view(|_window, _cx| BlankView);

    // 未知类型（没有 badge）、没有列筛选：表头块 = 列名 + 排序槽，没有 gap 参与
    let long_name = "order_identifier_column";
    let mut grid = ResultGridDelegate::empty("尚未执行");
    grid.set_data(
        vec![long_name.to_string(), "b".to_string()],
        vec![vec!["1".to_string(), "w".to_string()]],
    );
    let fitted = vcx.update(|window, cx| crate::grid::fit::fit_column(&grid, 1, window, cx));
    let fitted = fitted.expect("数据列应自适应");

    // 同一套测宽口径量列名本身（同字体、同字号、同换行处理）
    let name_w = vcx.update(|window, _cx| {
        let style = window.text_style();
        crate::grid::fit::measure(
            window,
            long_name,
            &style.font(),
            style.color,
            window.rem_size() * 0.75,
        )
    });

    // 前提：列名长到表头压过单元格（否则这条等式不成立）
    assert!(
        fitted > px(80.),
        "列名这么长时表头应主导宽度：{fitted:?}"
    );
    assert_eq!(
        fitted,
        name_w + RESULT_FIT_HEADER_CHROME,
        "自适应宽 = 列名 + 排序图标槽（{name_w:?} + {RESULT_FIT_HEADER_CHROME:?}）"
    );
}

/// 【类型默认列宽】没拖过、也没双击自适应过的列按**列类型**给初值；拖过的一律按用户宽度
/// （类型档不覆盖用户选择）；未知类型仍是 128（守住「类型缺失时一个像素都不变」）
#[gpui_kit::test]
fn default_column_width_follows_type_until_the_user_drags(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);

    // 1) 纯函数口径：11 档全覆盖（`default_column_width` 是穷尽 match，漏一档编译不过）
    for (kind, expected) in [
        (ColumnKind::Integer, crate::ui::RESULT_COLUMN_WIDTH_NUMERIC),
        (ColumnKind::Float, crate::ui::RESULT_COLUMN_WIDTH_NUMERIC),
        (ColumnKind::Decimal, crate::ui::RESULT_COLUMN_WIDTH_NUMERIC),
        (ColumnKind::Boolean, crate::ui::RESULT_COLUMN_WIDTH_BOOLEAN),
        (ColumnKind::Timestamp, crate::ui::RESULT_COLUMN_WIDTH_TIMESTAMP),
        (ColumnKind::Date, crate::ui::RESULT_COLUMN_WIDTH_DATE_TIME),
        (ColumnKind::Time, crate::ui::RESULT_COLUMN_WIDTH_DATE_TIME),
        (ColumnKind::Uuid, crate::ui::RESULT_COLUMN_WIDTH_UUID),
        (ColumnKind::Json, crate::ui::RESULT_COLUMN_WIDTH_JSON),
        (ColumnKind::Binary, crate::ui::RESULT_COLUMN_WIDTH_BINARY),
        (ColumnKind::Text, crate::ui::RESULT_COLUMN_WIDTH_TEXT),
        (ColumnKind::Unknown, crate::ui::RESULT_COLUMN_WIDTH),
    ] {
        assert_eq!(
            crate::grid::table_impl::default_column_width(kind),
            expected,
            "{kind:?} 的默认宽不符"
        );
    }

    // 2) 走真实列定义（列宽是从 `column()` 里取的，这里钉的是「类型 → 列宽」那一段）
    let mut typed = grid();
    typed.set_column_types(vec![
        Some(ColumnType::parse("uuid")),
        Some(ColumnType::parse("boolean")),
    ]);
    cx.update(|cx: &mut App| {
        assert_eq!(
            typed.column(1, cx).width,
            crate::ui::RESULT_COLUMN_WIDTH_UUID,
            "uuid 列走 300"
        );
        assert_eq!(
            typed.column(2, cx).width,
            crate::ui::RESULT_COLUMN_WIDTH_BOOLEAN,
            "boolean 列走布尔档（120，按表头 badge + 列名 + 列头菜单算出来的）"
        );
    });

    // 3) 拖过之后按用户宽度，类型档不再生效
    //    注意 70 < `RESULT_COLUMN_MIN_WIDTH`（80）：列宽会被最小宽兜底，这是组件的
    //    `Column::min_width` 语义，不是拖拽没生效——2026-11-08 把最小宽从 64 提到 80 后
    //    这里就从「等于用户值」变成「等于最小宽」，故意保留 70 来钉住这条兜底行为。
    typed.set_column_widths(&[
        crate::ui::RESULT_ROW_NUMBER_WIDTH,
        gpui_kit::px(200.),
        gpui_kit::px(70.),
    ]);
    let expected_boolean = {
        let dragged = gpui_kit::px(70.);
        if dragged < crate::ui::RESULT_COLUMN_MIN_WIDTH {
            crate::ui::RESULT_COLUMN_MIN_WIDTH
        } else {
            dragged
        }
    };
    cx.update(|cx: &mut App| {
        assert_eq!(
            typed.column(1, cx).width,
            gpui_kit::px(200.),
            "uuid 列拖过按用户值"
        );
        assert_eq!(
            typed.column(2, cx).width,
            expected_boolean,
            "boolean 列拖过按用户值（低于最小宽时被兜底到最小宽）"
        );
    });
}

/// 【窄档列宽回归 2026-11-08】默认列宽档与最小列宽都必须给**列名**留出可读空间。
///
/// 列宽真正要装的是表头，不是值：表头固定占宽上界 = 组件左右 padding 8 + 类型 badge
/// 19（11px 单字符 + `px_1`）+ badge / 列名之间一个 `gap_1` 4 = **35**。
/// （列头菜单从 2026-11-08 起是绝对定位、悬停才显形，**不计入**这一份——它此前并进主行
/// 流内恒吃 24px，是窄列列名被挤没的主因。）
/// 96px 的旧布尔档扣掉 51 只剩 45（5–6 个西文字符，`is_active` 会变 `is_a…`）；
/// 64px 的旧最小宽更只剩 13px，连省略号都摆不下。这条用例把数字钉住，改回去立刻挂。
#[test]
fn narrow_column_tiers_keep_the_header_readable() {
    const HEADER_FIXED: gpui_kit::Pixels = gpui_kit::px(35.);
    // 8 个 12px 西文字符 ≈ 56px——「能认出列名」的下限
    const READABLE: gpui_kit::Pixels = gpui_kit::px(56.);

    let boolean_room = crate::ui::RESULT_COLUMN_WIDTH_BOOLEAN - HEADER_FIXED;
    assert!(
        boolean_room >= READABLE,
        "布尔列默认档 {:?} 扣掉表头固定占宽后只剩 {:?} 给列名（要求 >= {:?}）",
        crate::ui::RESULT_COLUMN_WIDTH_BOOLEAN,
        boolean_room,
        READABLE
    );

    // 最小宽不要求「能读」，但至少要留出一段非零空间——列名不能归零
    let min_room = crate::ui::RESULT_COLUMN_MIN_WIDTH - HEADER_FIXED;
    assert!(
        min_room >= gpui_kit::px(16.),
        "最小列宽 {:?} 扣掉表头固定占宽后只剩 {:?}，列名被挤成一条缝",
        crate::ui::RESULT_COLUMN_MIN_WIDTH,
        min_room
    );

    // 其余默认档都比布尔档宽，天然满足；这里只确认没有哪一档掉到布尔档以下
    for (label, width) in [
        ("numeric", crate::ui::RESULT_COLUMN_WIDTH_NUMERIC),
        ("date/time", crate::ui::RESULT_COLUMN_WIDTH_DATE_TIME),
        ("timestamp", crate::ui::RESULT_COLUMN_WIDTH_TIMESTAMP),
        ("uuid", crate::ui::RESULT_COLUMN_WIDTH_UUID),
        ("json", crate::ui::RESULT_COLUMN_WIDTH_JSON),
        ("binary", crate::ui::RESULT_COLUMN_WIDTH_BINARY),
        ("text", crate::ui::RESULT_COLUMN_WIDTH_TEXT),
    ] {
        assert!(
            width - HEADER_FIXED >= READABLE,
            "{label} 档 {:?} 给列名的空间不足",
            width
        );
    }
}

/// 只为把表格真正渲染进窗口的探针视图（几何必须来自真实布局）
struct HeaderHarness {
    state: gpui_kit::Entity<gpui_kit::component::table::TableState<ResultGridDelegate>>,
}

impl gpui_kit::Render for HeaderHarness {
    fn render(
        &mut self,
        _window: &mut gpui_kit::Window,
        cx: &mut gpui_kit::Context<Self>,
    ) -> impl gpui_kit::IntoElement {
        // 走生产路径：表头带高由 delegate 决定（注释开启 → 40px，单行 → 组件默认 26px）
        let header_height = self.state.read(cx).delegate().header_row_height();
        let table = gpui_kit::component::table::DataTable::new(&self.state)
            .with_size(crate::ui::RESULT_TABLE_SIZE);
        let table = match header_height {
            Some(height) => table.header_row_height(height),
            None => table,
        };
        gpui_kit::div().size_full().child(table)
    }
}

/// 【表头遮挡回归 2026-11-08】表头带高 = 组件 `table_row_height()`（本仓 `RESULT_TABLE_SIZE`
/// = XSmall 档 = 26px，减上下 padding 4px 只剩 **22px** 内容高）。列头菜单曾经是主行的
/// **兄弟**——同一个 `v_flex` 里的第二行；两行高度加起来远超 22px，flex 先把 28px 的菜单排满，
/// 主行被挤成 0 高：表头只剩 badge 的半截彩色残线，列名整行看不见（用户截图实证）。
///
/// 修复后菜单并进主行（`flex_none`），这里锁住「主行至少装得下一行文字」——
/// 旧实现下 `main.size.height` 是 0，断言必挂。
#[gpui_kit::test]
fn header_main_row_keeps_a_readable_height(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let (_, vcx) = cx.add_window_view(|window, cx| HeaderHarness {
        state: cx.new(|cx| gpui_kit::component::table::TableState::new(grid(), window, cx)),
    });

    // 首帧只排位置，几何第二帧才稳定（配 retained 渲染内核尤其）
    for _ in 0..2 {
        vcx.update(|window, cx| {
            window.draw(cx).clear(cx);
        });
    }

    let main = vcx
        .debug_bounds("editor-result-th-main-1")
        .expect("表头主行应有 debug 几何（表格没渲染出来时这里为空）");

    // 表头带的下边线挂在 `render_header` 的 hook 上（本仓唯一一处给表头加边框的地方）。
    // 组件若不调用这个 hook，边框会静默缺失而与「组件没这入口」分不清——先坐实它跑了。
    assert!(
        vcx.debug_bounds("editor-result-header-band").is_some(),
        "表头容器 hook 没被调用：DBeaver 的表头分隔线不会落地"
    );
    assert!(
        main.size.height >= gpui_kit::px(14.),
        "表头主行被挤扁，列名会被裁掉：main={:?}",
        main.size.height
    );
    // 表头带 26px、上下 padding 4px → 主行不可能高过 22px
    assert!(
        main.size.height <= gpui_kit::px(22.),
        "主行不该高过表头内容区：main={:?}",
        main.size.height
    );

    // 列头菜单必须与列名**同一行**：它一旦落成主行的兄弟（第二行），22px 内容高
    // 先被菜单排满，主行就被挤成 0——这正是被遮挡的形态。
    // 2026-11-08 起菜单是绝对定位 + 悬停才显形（默认 `invisible`），观测可能不登记
    // 不可见元素：取不到就跳过几何校验（"不占位"由
    // `header_column_menu_floats_and_leaves_the_name_its_full_width` 正面钉住）。
    if let Some(menu) = vcx.debug_bounds("editor-result-col-menu-1") {
        let offset = (menu.origin.y - main.origin.y).abs();
        assert!(
            offset < gpui_kit::px(4.),
            "列头菜单落到了第二行：main.y={:?} menu.y={:?}",
            main.origin.y,
            menu.origin.y
        );
        // 贴右端：绝对定位后它应该压在主行右边缘附近，而不是另起一行
        let right_gap = (main.origin.x + main.size.width) - (menu.origin.x + menu.size.width);
        assert!(
            right_gap.abs() <= gpui_kit::px(2.),
            "列头菜单没贴主行右端：main.right={:?} menu.right={:?}",
            main.origin.x + main.size.width,
            menu.origin.x + menu.size.width
        );
    }
}

/// 【结果集配色槽 2026-11-08】奇数行的斑马底色由 delegate 自己的行容器（`render_tr`）
/// 铺 DBeaver `cell.odd.background` 同款浅底，而不是组件库 `.stripe(true)` 的全局
/// `tokens.table_even`。
///
/// headless 下读不到颜色，这里只锁**分支真的跑了**：`editor-result-row-odd` 是奇数行
/// 专属哨兵，取不到它说明 `render_tr` 没被调用（或表根本没渲染出来）——那样这个槽就是
/// 空转，颜色改了一整轮却不上屏。颜色覆盖链的源码依据见 `table_impl.rs::render_tr` 注释。
#[gpui_kit::test]
fn odd_rows_paint_the_stripe_from_the_delegate_row_container(
    cx: &mut gpui_kit::TestAppContext,
) {
    cx.update(gpui_kit::init);
    let (_, vcx) = cx.add_window_view(|window, cx| HeaderHarness {
        state: cx.new(|cx| gpui_kit::component::table::TableState::new(grid(), window, cx)),
    });

    // 首帧只排位置，几何第二帧才稳定（配 retained 渲染内核尤其）
    for _ in 0..2 {
        vcx.update(|window, cx| {
            window.draw(cx).clear(cx);
        });
    }

    let odd = vcx
        .debug_bounds("editor-result-row-odd")
        .expect("奇数行应有 delegate 行容器（取不到 → render_tr 没被调用，配色槽空转）");
    assert!(
        odd.size.height >= gpui_kit::px(10.),
        "奇数行容器应有真实高度：odd={:?}",
        odd.size.height
    );
}

/// 【选中行 2026-11-18】组件自己**不给**选中行铺色（vendor `table/state.rs:2249` 的
/// `is_selected` 只决定行底线要不要画、hover 要不要抑制），于是「再点一下已选中的格子」
/// 升级成整行选中（`grid/status.rs` 关掉了组件窄行头，这是选整行的唯一入口）时，
/// 网格上一个像素都不变。DBeaver 的选中行是整行色（`LightGrid` 逐格按
/// `getCellBackground(selected)` 取色），所以由 delegate 行容器整行铺
/// `tokens.table_active`（与「选中单元格」同槽，两者不会各说各话）。
///
/// headless 读不到颜色，锁两件事：① 选中行分支真的跑了（`editor-result-row-selected`
/// 哨兵，取不到就是 `render_tr` 没走选中分支）；② 选中行**压掉**斑马纹（同一条奇数行上
/// `editor-result-row-odd` 必须消失——同一行只能铺一档色，否则两层叠加出偏色）。
#[gpui_kit::test]
fn selected_row_paints_a_whole_row_band(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);

    // 选中第 1 行：它本身是奇数行（有斑马纹），正好验证「选中压掉斑马」
    let mut delegate = grid_with(&[&["1", "a"], &["2", "b"], &["3", "c"]]);
    delegate.set_selected_row(Some(1));
    assert_eq!(
        delegate.selected_view_row(),
        Some(1),
        "delegate 应记住组件报来的整行选中"
    );
    let (_, vcx) = cx.add_window_view(|window, cx| HeaderHarness {
        state: cx.new(|cx| gpui_kit::component::table::TableState::new(delegate, window, cx)),
    });

    // 首帧只排位置，几何第二帧才稳定（配 retained 渲染内核尤其）
    for _ in 0..2 {
        vcx.update(|window, cx| {
            window.draw(cx).clear(cx);
        });
    }

    let selected = vcx
        .debug_bounds("editor-result-row-selected")
        .expect("选中行应由 delegate 行容器铺色（取不到 → render_tr 没走选中分支）");
    assert!(
        selected.size.height >= gpui_kit::px(10.),
        "选中行容器应有真实高度：selected={:?}",
        selected.size.height
    );
    assert!(
        vcx.debug_bounds("editor-result-row-odd").is_none(),
        "选中的那一行不能再走斑马分支（同一行只能铺一档色）"
    );

    // 换结果集：上一份的行号不该跟到新数据上（否则新结果凭空多一行高亮）
    let mut reused = grid_with(&[&["1", "a"], &["2", "b"], &["3", "c"]]);
    reused.set_selected_row(Some(1));
    reused.set_data(
        vec!["id".to_string(), "name".to_string()],
        vec![vec!["9".to_string(), "z".to_string()]],
    );
    assert_eq!(
        reused.selected_view_row(),
        None,
        "换了一份结果，旧的整行选中应作废"
    );
}

/// 【内距单一来源 2026-11-18】渲染侧的「单侧 6px」与自适应侧的「合计 12px」必须是
/// 同一个尺寸：`render_td` / `render_th` 读 `RESULT_CELL_PADDING_X`（单侧），
/// `fit_col` 读 `RESULT_FIT_CELL_PADDING`（合计）。两者分叉时，双击自适应算出的列宽
/// 会和实际渲染差一整个边距，列名被省略号吃掉一半——2026-11-17 之前渲染写的是
/// `rems(0.375)`（随主题字号浮动）、自适应写的是 `px(12.)`（固定），就埋着这个雷。
///
/// `Pixels` 的字段是 `pub(crate)`，const 上下文写不了乘法，所以两个常量只能各写各的字面量；
/// 这条用例就是替那个写不出来的 `= 单侧 × 2` 兜底。
#[test]
fn cell_padding_single_source_matches_fit_padding() {
    use crate::ui::{RESULT_CELL_PADDING_X, RESULT_FIT_CELL_PADDING};

    assert_eq!(
        RESULT_FIT_CELL_PADDING,
        RESULT_CELL_PADDING_X * 2.,
        "自适应用的合计内距必须等于渲染用的单侧内距 × 2"
    );
}

/// 【行号槽底色 2026-11-18】DBeaver 每个行头格都铺表头底色
/// （`GridRowRenderer.java:55-70` 逐行 `getHeaderBackground(element, selected)`），
/// 行号列因此读起来是一条独立的浅色竖条。本仓同口径，但「选中行不铺」——
/// 整行选中色是半透明的、已经铺在行容器上，行号格再铺一层同色会叠成两倍透明度。
///
/// 判据抽成纯函数 + delegate 包装（组合由它们定，渲染只照着做），这里锁组合，
/// 并且走 delegate 真状态（`set_selected_row` 之后同一行必须翻成「不铺」）。
/// headless 读不到颜色，所以不锁色值——色走 `theme.tokens.table_head`（主题单一来源）。
#[test]
fn row_number_gutter_paints_the_header_background() {
    // 纯判据：未选中铺表头色；选中行留空（让整行色透上来）；本地行保留身份色
    assert!(crate::grid::cell::row_number_paints_head(false, false));
    assert!(!crate::grid::cell::row_number_paints_head(false, true));
    assert!(!crate::grid::cell::row_number_paints_head(true, false));

    // delegate 包装：同一行被整行选中后，判据翻成「不铺」
    let mut delegate = grid_with(&[&["1", "a"], &["2", "b"]]);
    assert!(delegate.row_number_paints_head(0), "未选中时应铺表头底色");
    delegate.set_selected_row(Some(0));
    assert!(
        !delegate.row_number_paints_head(0),
        "整行选中后行号格必须留空（否则半透明选中色叠两层、比数据格深）"
    );
    assert!(
        delegate.row_number_paints_head(1),
        "没被选中的另一行照旧铺表头底色"
    );
    assert!(
        delegate.row_number_is_bold(0),
        "整行选中行 = 当前行，行号要加粗（DBeaver GridRowRenderer:96）"
    );
    assert!(
        !delegate.row_number_is_bold(1),
        "非当前行的行号保持常规字重"
    );
}

/// 【注释表头 2026-11-08】注释开关开启时表头是**两行**（列名主行 + 注释小字），
/// 但表头带高一直跟着组件的行高档走（XSmall = 26px，扣 padding 只剩 22px 内容高）：
/// 两行挤 22px 的结果是各被 `overflow_hidden` 裁掉一半，主行读不出字。
///
/// 修复：`ResultGridDelegate::header_row_height()` 在这**一档**返回 40px（内容高 36 =
/// 两行各 18px），宿主把它交给 `DataTable::header_row_height`。单行表头返回 `None`，
/// 一个像素都不动。
#[gpui_kit::test]
fn comment_header_row_gets_room_for_two_lines(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);

    // 1) 纯逻辑：只有注释开关会抬表头
    let mut plain = grid();
    assert_eq!(
        plain.header_row_height(),
        None,
        "单行表头不该覆盖组件默认高度（默认视觉必须一个像素都不变）"
    );
    plain.set_show_column_comments(true);
    assert_eq!(
        plain.header_row_height(),
        Some(crate::ui::RESULT_HEADER_COMMENT_ROW_HEIGHT),
        "注释开启后表头是两行，必须给覆盖高度"
    );

    // 2) 几何：真渲染两帧后两行都上屏、且不重叠
    let annotation = ColumnAnnotation {
        source_table: None,
        source_column: None,
        comment: Some("总额".into()),
        data_type: None,
        nullable: None,
        default_value: None,
    };
    let (_, vcx) = cx.add_window_view(|window, cx| HeaderHarness {
        state: cx.new(|cx| {
            let mut state = gpui_kit::component::table::TableState::new(grid(), window, cx);
            state.delegate_mut().set_show_column_comments(true);
            state.delegate_mut().set_column_annotations(vec![
                Some(annotation.clone()),
                Some(annotation),
            ]);
            state
        }),
    });
    for _ in 0..2 {
        vcx.update(|window, cx| {
            window.draw(cx).clear(cx);
        });
    }

    let main = vcx
        .debug_bounds("editor-result-th-main-1")
        .expect("表头主行应有 debug 几何");
    let comment = vcx
        .debug_bounds("editor-result-th-comment-1")
        .expect("注释开关开着、列有注释时注释行必须上屏（被裁的形态下这里取不到）");
    assert!(
        main.size.height >= gpui_kit::px(14.),
        "抬高表头后主行仍须装下一行文字：main={:?}",
        main.size.height
    );
    assert!(
        comment.size.height >= gpui_kit::px(12.),
        "注释行被裁成一条缝：comment={:?}",
        comment.size.height
    );
    assert!(
        comment.origin.y > main.origin.y,
        "注释行应落主行下方：main.y={:?} comment.y={:?}",
        main.origin.y,
        comment.origin.y
    );
}

/// 【列头菜单不再占列宽 2026-11-08】菜单此前并进主行流内，恒吃 24px：布尔列 96px 时
/// 列名只剩 37px（`is_active` → `is_a…`）。改成绝对定位贴右端 + 悬停表头才显形之后，
/// 列名拿整列宽。
///
/// 几何口径：`grid()` 两列都是未知类型（无 badge、无排序箭头、无筛选 ▽），默认列宽
/// 128 扣掉组件左右 padding 8、再扣掉组件 `render_th` 自带的排序图标槽（实测 16）→
/// 列名应拿到 **104**。菜单若回到流内（旧实现）只剩 80，断言 >= 96 足以区分两代实现。
///
/// 2026-11-18：表头补了 1px 列边界线（画在格的左边界，占掉 1px 内容宽）→ 实测 95，
/// 阈值下调到 **92**：仍远高于「菜单在流内」的 80，区分力不变。
#[gpui_kit::test]
fn header_column_menu_floats_and_leaves_the_name_its_full_width(
    cx: &mut gpui_kit::TestAppContext,
) {
    cx.update(gpui_kit::init);
    let (_, vcx) = cx.add_window_view(|window, cx| HeaderHarness {
        state: cx.new(|cx| gpui_kit::component::table::TableState::new(grid(), window, cx)),
    });
    for _ in 0..2 {
        vcx.update(|window, cx| {
            window.draw(cx).clear(cx);
        });
    }

    let name = vcx
        .debug_bounds("editor-result-th-name-1")
        .expect("列名应有 debug 几何");
    let main = vcx
        .debug_bounds("editor-result-th-main-1")
        .expect("表头主行应有 debug 几何");

    assert!(
        name.size.width >= gpui_kit::px(92.),
        "列名没拿到整列宽（列头菜单又回到流内占位了？）：name={:?} main={:?}",
        name.size.width,
        main.size.width
    );
    // 列名是主行里唯一的 flex 子元素：它就该铺满主行（差值只剩 `gap_1` 的余量）
    assert!(
        main.size.width - name.size.width <= gpui_kit::px(8.),
        "列名没有铺满主行，流里还有别的占宽元素：main={:?} name={:?}",
        main.size.width,
        name.size.width
    );
}

/// 【网格线 2026-11-17】列内距改由**单元格自己**画 6px、组件层置零（
/// `grid/table_impl.rs::cell_paddings`）：单元格因此撑满整列宽，那条 1px 竖分隔线才落在
/// 列边界上（DBeaver `LightGrid` 的 `linesVisible` 默认开、逐列画线，`LightGrid.java:295`
/// 与 `:2425-2439`）。
///
/// 断言用**列宽常量**当基准：改前组件那层左右各 4px 内距会把单元格挤窄 8px（行号槽
/// 48 → 40、数据列 128 → 120）。谁把左右内距又填回列定义里，这里立刻掉下来。
///
/// 表头走同一条几何（2026-11-18 补）：表头的列边界线画在左边界（右端被排序图标槽
/// 占 20px），所以断言收窄为「行号列表头撑满列宽」+「数据列表头从行号槽右缘开始」。
#[gpui_kit::test]
fn cells_stretch_to_the_column_boundary_for_grid_lines(cx: &mut gpui_kit::TestAppContext) {
    use gpui_kit::px;

    cx.update(gpui_kit::init);
    let (_, vcx) = cx.add_window_view(|window, cx| HeaderHarness {
        state: cx.new(|cx| gpui_kit::component::table::TableState::new(grid(), window, cx)),
    });
    for _ in 0..2 {
        vcx.update(|window, cx| {
            window.draw(cx).clear(cx);
        });
    }

    let rowno = vcx
        .debug_bounds("editor-result-rowno-0")
        .expect("行号格应有 debug 几何");
    let cell = vcx
        .debug_bounds("editor-result-cell-0-1")
        .expect("数据格应有 debug 几何");

    // 行号槽：列宽 48px 扣掉组件 8px 只剩 40（旧行为），撑满时是 48
    assert!(
        rowno.size.width >= crate::ui::RESULT_ROW_NUMBER_WIDTH - px(1.),
        "行号格没撑满列宽（组件那层内距又回来了？）：rowno={:?}",
        rowno.size.width
    );
    // 数据列同理：128 vs 120（留 4px 容差给分数像素对齐）
    assert!(
        cell.size.width >= crate::ui::RESULT_COLUMN_WIDTH - px(4.),
        "数据格没撑满列宽（竖分隔线会画在列内侧 6px 处）：cell={:?}",
        cell.size.width
    );

    // 表头：列边界线画在表头格的**左**边界（右端被排序图标槽占 20px，
    // `state.rs:1910`），所以这里钉的是「数据列表头从行号槽右缘开始」——
    // 它一偏，表头竖线就与数据区的竖线错位。
    let th0 = vcx
        .debug_bounds("editor-result-th-0")
        .expect("行号列表头应有 debug 几何");
    let th1 = vcx
        .debug_bounds("editor-result-th-1")
        .expect("数据列表头应有 debug 几何");
    assert!(
        th0.size.width >= crate::ui::RESULT_ROW_NUMBER_WIDTH - px(4.),
        "行号列表头没撑满列宽（表头竖线会落到列内侧）：th0={:?}",
        th0.size.width
    );
    assert!(
        th1.left() - th0.left() >= crate::ui::RESULT_ROW_NUMBER_WIDTH - px(1.),
        "数据列表头没从行号槽右缘开始（表头竖线与数据区竖线错位）：th0={:?} th1={:?}",
        th0,
        th1
    );
}

/// 【行号槽右键 2026-11-21】右键行号格必须把它自己记成菜单目标（`context_cell`）。
///
/// 组件的右键派发只认 delegate 上**最近一次**落点（`grid/menu.rs::context_menu` 读
/// `delegate.context_cell`），行号列若不挂自己的 `on_mouse_down(Right)`，右键行号槽
/// 会沿用上一个数据格的落点——菜单回头去摆那一列的内容，功能却叫「复制行号」。
///
/// 这条钉的是**交互链路**（真事件 → 元素回调 → delegate 状态）；菜单形态由
/// `row_number_menu_offers_copying_the_visible_row_number` 钉，两者互补：
/// 形态对了但链路没接，屏幕上依然是空动作。
#[gpui_kit::test]
fn right_clicking_the_row_number_cell_records_it_as_the_menu_target(
    cx: &mut gpui_kit::TestAppContext,
) {
    use gpui_kit::{MouseButton, Modifiers};

    cx.update(gpui_kit::init);
    let (harness, vcx) = cx.add_window_view(|window, cx| HeaderHarness {
        state: cx.new(|cx| gpui_kit::component::table::TableState::new(grid(), window, cx)),
    });

    // 首帧只排位置，几何第二帧才稳定（配 retained 渲染内核尤其）
    for _ in 0..2 {
        vcx.update(|window, cx| {
            window.draw(cx).clear(cx);
        });
    }

    let rowno = vcx
        .debug_bounds("editor-result-rowno-0")
        .expect("行号格应有 debug 几何");

    // 先移动再按下：命中测试读窗口记录的最近一次指针位置，不先 move 时按下可能落在
    // 上一个指针位置（用例里是原点）。
    vcx.simulate_mouse_move(rowno.center(), MouseButton::Right, Modifiers::default());
    vcx.simulate_mouse_down(rowno.center(), MouseButton::Right, Modifiers::default());

    let recorded = vcx.update(|_window, cx| {
        harness
            .read(cx)
            .state
            .read(cx)
            .delegate()
            .context_cell
    });
    assert_eq!(
        recorded,
        Some((0, 0)),
        "右键行号格没记成菜单目标（行号列 0 / 行号槽列传 0）：菜单会沿用上一个数据格的落点"
    );
}

/// 只为把工具栏真正渲染进窗口的探针视图（几何必须来自真实布局）
struct ToolbarHarness;

impl gpui_kit::Render for ToolbarHarness {
    fn render(
        &mut self,
        _window: &mut gpui_kit::Window,
        cx: &mut gpui_kit::Context<Self>,
    ) -> impl gpui_kit::IntoElement {
        // 走生产路径：左段留空（右段已在 T1 收口移除），量的就是容器自己写死的高与底色
        render_toolbar(None, cx)
    }
}

/// 【工具栏高度 2026-11-08】工具栏高 = 标签条高 = 2rem/32px，两块连成一条 chrome 带。
///
/// 旧值 40px 的来历：筛选框 `Input` 还是 Medium 档（`h_8` 32px）+ Toolbar `p_1` 上下
/// 8px = 40；`Input` 降到 `.small()`（24px）后这 8px 没回收，工具栏比标签条虚胖一圈
/// （用户实拍指出「工具栏没对齐 DBeaver，背景、高度等等」）。
///
/// 断言写**绝对值 32**（默认主题字号 16px 下的 `rems(2.0)`）而不是「等于 tabs 常量」：
/// 两个常量一旦有人各自改档，绝对值这条先红。
#[gpui_kit::test]
fn the_result_toolbar_keeps_the_tabs_height(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let (_, vcx) = cx.add_window_view(|_window, _cx| ToolbarHarness);

    // 首帧只排位置，几何第二帧才稳定（配 retained 渲染内核尤其）
    for _ in 0..2 {
        vcx.update(|window, cx| {
            window.draw(cx).clear(cx);
        });
    }

    let bar = vcx
        .debug_bounds("editor-result-toolbar")
        .expect("工具栏应有 debug 几何（没渲染出来时这里为空）");
    let height = bar.size.height.as_f32();
    assert!(
        (height - 30.0).abs() < 0.5,
        "工具栏高应为 30px（DBeaver `3 + 2 + 20 + 2 + 3`；本仓 20px 控件 + 24px 筛选区 + 上下各 3px），实得 {height}"
    );
}

/// 【工具栏配置】状态栏配置菜单的纯函数口径：顺序 = DBeaver 底部工具条顺序，
/// 默认勾选 = `plugin.xml:1381-1397` 的 `defaultVisibility`（撤销 / 重做为 false；
/// `fetch.page` 是有意偏离：本仓「取下一段」是窗口化取数的唯一入口，保持常显）
#[test]
fn status_config_items_follow_dbeaver_toolbar_order_and_defaults() {
    use crate::grid::status_config_items;
    use crate::ports::StatusCommand;

    // 默认隐藏集合由 `default_visible` 折算
    let hidden: std::collections::BTreeSet<StatusCommand> = StatusCommand::ALL
        .into_iter()
        .filter(|command| !command.default_visible())
        .collect();
    assert_eq!(hidden.len(), 2, "默认隐藏两项（撤销 / 重做）");
    assert!(!StatusCommand::Undo.default_visible());
    assert!(!StatusCommand::Redo.default_visible());
    assert!(
        StatusCommand::FetchPage.default_visible(),
        "取下一段有意偏离 DBeaver：本仓它是取数的唯一可见入口"
    );
    assert!(StatusCommand::Commit.default_visible());
    assert!(StatusCommand::Export.default_visible());

    let items = status_config_items(&hidden);
    assert_eq!(items.len(), StatusCommand::ALL.len(), "每个命令一项");
    assert_eq!(
        items.iter().map(|(command, _, _)| *command).collect::<Vec<_>>(),
        StatusCommand::ALL.to_vec(),
        "菜单顺序与 DBeaver 工具条命令顺序一致"
    );
    for (command, label, checked) in &items {
        assert!(!label.is_empty(), "{command:?} 的文案不能为空");
        assert_eq!(
            *checked,
            command.default_visible(),
            "{command:?} 的勾选态应等于 defaultVisibility"
        );
    }

    // 用户勾回撤销：该项变勾选，其余不变
    let mut restored = hidden.clone();
    restored.remove(&StatusCommand::Undo);
    let items = status_config_items(&restored);
    assert!(
        items
            .iter()
            .any(|(command, _, checked)| *command == StatusCommand::Undo && *checked),
        "撤销勾回后应为勾选"
    );
    assert!(
        items
            .iter()
            .any(|(command, _, checked)| *command == StatusCommand::Redo && !*checked),
        "重做仍保持未勾选"
    );
}

/// 【工具栏配置落盘】命令 id 与落盘文本的往返口径（settings 里存的就是这些 id）：
/// id 全局唯一、`from_id` 与 `id` 互逆、解析忽略空白项与未知 id、序列化按 `ALL` 顺序稳定。
/// 出厂文本 `undo,redo` 必须解析出「撤销 + 重做」两项（与 `plugin.xml:1381-1397`
/// 的 `defaultVisibility=false` 同集合）。
#[test]
fn status_command_ids_round_trip_and_parse_the_persisted_list() {
    use crate::ports::StatusCommand;
    use std::collections::BTreeSet;

    // id 唯一且非空；from_id 与 id 互逆
    let mut ids = BTreeSet::new();
    for command in StatusCommand::ALL {
        let id = command.id();
        assert!(!id.is_empty(), "{command:?} 的 id 不能为空");
        assert!(ids.insert(id), "id 重复：{id}");
        assert_eq!(StatusCommand::from_id(id), Some(command), "{id} 应能反查回 {command:?}");
    }
    assert_eq!(StatusCommand::from_id("no-such-command"), None);

    // 解析：容忍空格 / 空项 / 未知 id，大小写按落盘口径（小写）
    let parsed = StatusCommand::parse_hidden_list(" undo , , redo ,no-such-command");
    let expected: BTreeSet<StatusCommand> =
        [StatusCommand::Undo, StatusCommand::Redo].into_iter().collect();
    assert_eq!(parsed, expected, "未知 id 与空项应被忽略");
    assert_eq!(
        StatusCommand::parse_hidden_list("undo,redo"),
        [StatusCommand::Undo, StatusCommand::Redo].into_iter().collect(),
        "出厂文本 `undo,redo` = 撤销 + 重做"
    );
    assert!(StatusCommand::parse_hidden_list("").is_empty());

    // 序列化：顺序跟 ALL（与用户勾选顺序无关），空集合 → 空串
    let mut reversed: BTreeSet<StatusCommand> =
        [StatusCommand::Refresh, StatusCommand::Undo, StatusCommand::Commit]
            .into_iter()
            .collect();
    assert_eq!(
        StatusCommand::hidden_list_text(&reversed),
        "commit,undo,refresh",
        "按 ALL 顺序输出，保证同一集合序列化稳定"
    );
    reversed.clear();
    assert_eq!(StatusCommand::hidden_list_text(&reversed), "");

    // 往返：序列化 → 解析必须回到同一集合
    let mut sample = expected.clone();
    sample.insert(StatusCommand::Export);
    assert_eq!(
        StatusCommand::parse_hidden_list(&StatusCommand::hidden_list_text(&sample)),
        sample
    );
}
// ============================ 【第 49 批】选中统计三项 ============================
// DBeaver 取证：`ResultSetViewer.java:3580-3588`（`fillLayoutMenu` 里的三颗
// `ToggleSelectionStatAction`）、`ResultSetStatListener.java:34-95`（100ms 防抖后按三项
// 偏好用 `StringJoiner(", ")` 拼）、`ResultSetMessages_zh.properties:633-637`
// （`行数: {0}` / `列数: {0}` / `单元格数: {0}`，半角冒号 + 空格）。
// 本仓差异：不做防抖（事件由组件批量发）、菜单扁平无子菜单（三颗平铺在右键尾部）。

/// 【DBeaver 对齐】状态行统计段纯函数：只拼启用项、顺序固定 行 → 列 → 单元格、
/// 项间 `", "`（`StringJoiner(", ")`），文案照中文包；三项全关 → 空串（状态行不摆）。
///
/// 0 也照样拼：DBeaver 在没有选中 region 时三项都记 0 并仍加入 joiner
/// （`ResultSetStatListener.java:80-90`），不是「没选中就整段消失」——整段消失只由
/// 三项偏好全关决定。
#[test]
fn selection_statistics_joins_enabled_items_in_fixed_order() {
    let status = ResultStatus {
        total_rows: 2,
        selected_rows: 2,
        selected_columns: 3,
        selected_cells: 6,
        ..ResultStatus::default()
    };

    // 出厂态：三项全关 → 空串（用户 2026-10-28「状态栏只显示内存 / 耗时 / 条数」不受影响）
    assert_eq!(
        selection_statistics(&status, SelectionStatPrefs::default()),
        ""
    );

    // 单项：只拼这一项，不带分隔符
    assert_eq!(
        selection_statistics(
            &status,
            SelectionStatPrefs {
                rows: true,
                ..SelectionStatPrefs::default()
            }
        ),
        "行数: 2"
    );

    // 全开：顺序固定 行 → 列 → 单元格（不被结构体字段顺序或 push 顺序带偏）
    assert_eq!(
        selection_statistics(
            &status,
            SelectionStatPrefs {
                rows: true,
                columns: true,
                cells: true,
            }
        ),
        "行数: 2, 列数: 3, 单元格数: 6"
    );

    // 只开后两项：仍按 列 → 单元格
    assert_eq!(
        selection_statistics(
            &status,
            SelectionStatPrefs {
                rows: false,
                columns: true,
                cells: true,
            }
        ),
        "列数: 3, 单元格数: 6"
    );

    // 没选中（三项计数 0）：开启的项照样摆 0
    assert_eq!(
        selection_statistics(
            &ResultStatus::default(),
            SelectionStatPrefs {
                rows: true,
                columns: true,
                cells: true,
            }
        ),
        "行数: 0, 列数: 0, 单元格数: 0"
    );

    // 千分位走本仓统一的 `thousands`（与状态行其它计数同口径）
    assert_eq!(
        selection_statistics(
            &ResultStatus {
                selected_rows: 1234,
                ..ResultStatus::default()
            },
            SelectionStatPrefs {
                rows: true,
                ..SelectionStatPrefs::default()
            }
        ),
        "行数: 1,234"
    );
}

/// 【DBeaver 对齐】右键 layout 段三颗开关：顺序固定 行 / 列 / 单元格，文案照中文包
/// （`ResultSetMessages_zh.properties:125/127/129` = 显示选定的行数 / 列数 / 单元格数），
/// 已开项带 `✓ ` 前缀（DBeaver 用 check box 表达同一件事），载荷 `on` 是点击后的目标态。
#[test]
fn selection_stat_menu_items_flag_current_state_and_flip_on_click() {
    let off = target("x", "id", 1);
    let items = selection_stat_menu_items(&off);
    assert_eq!(items.len(), 3);
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
    assert_eq!(
        labels,
        vec!["显示选定的行数", "显示选定的列数", "显示选定的单元格数"]
    );
    // 首项前带分隔线（DBeaver layout 段与上方命令之间有一条 separator），其余不带
    assert!(items[0].separator_before);
    assert!(!items[1].separator_before);
    assert!(!items[2].separator_before);
    // 当前全关：点一下就是「打开」
    for (item, kind) in items.iter().zip([
        SelectionStatKind::Rows,
        SelectionStatKind::Columns,
        SelectionStatKind::Cells,
    ]) {
        assert_eq!(
            item.action,
            ContextAction::ToggleSelectionStat { kind, on: true }
        );
    }

    // 已开的项带 `✓ ` 前缀，载荷是「关闭」
    let mut on = target("x", "id", 1);
    on.selection_stat_columns = true;
    on.selection_stat_cells = true;
    let items = selection_stat_menu_items(&on);
    assert_eq!(items[0].label, "显示选定的行数");
    assert_eq!(items[1].label, "✓ 显示选定的列数");
    assert_eq!(items[2].label, "✓ 显示选定的单元格数");
    assert_eq!(
        items[1].action,
        ContextAction::ToggleSelectionStat {
            kind: SelectionStatKind::Columns,
            on: false
        }
    );
    assert_eq!(
        items[2].action,
        ContextAction::ToggleSelectionStat {
            kind: SelectionStatKind::Cells,
            on: false
        }
    );
}

/// 【DBeaver 对齐】三颗开关的落点是数据格右键的**尾部**（`fillLayoutMenu` 挂在右键子菜单
/// `MENU_ID_LAYOUT`，`ResultSetViewer.java:3265-3270`）：本仓菜单扁平，所以它们紧排在这一落点
/// 的最后一段；而 DBeaver 在统计三颗之后还有「显示过滤器面板」（`fillLayoutMenu:3589-3592`），
/// 故这三颗是**倒数第 2～4 位**、筛选条显隐是最末项。
/// 列头箭头菜单 / 列浏览器清单不摆（DBeaver 的 layout 菜单只从表格右键进）。
#[test]
fn selection_stats_sit_at_the_context_menu_tail_and_not_in_column_menus() {
    let mut target = target_with_table("1", "id", 0);
    target.persist = true;
    let items = context_menu_items(&target);
    let tail: Vec<&str> = items[items.len() - 4..]
        .iter()
        .map(|i| i.label.as_str())
        .collect();
    assert_eq!(
        tail,
        vec![
            "显示选定的行数",
            "显示选定的列数",
            "显示选定的单元格数",
            "✓ 显示过滤器面板",
        ],
        "三颗选中统计开关应紧贴右键末尾（其后再跟过滤器面板开关）"
    );
    // 持久化项仍在这四项之前（layout 段是右键最后一段，不是插在中间）
    let persist_at = items
        .iter()
        .position(|i| matches!(i.action, ContextAction::PersistResultSet))
        .expect("可持久化落点应摆持久化项");
    assert!(persist_at < items.len() - 4);

    // 列头菜单不摆（它走另一条路径：DBeaver 列头箭头菜单里没有 layout 段）
    let column_target = target_with_table("1", "id", 0);
    let column_items = column_menu_items(&column_target);
    assert!(
        !column_items
            .iter()
            .any(|i| matches!(i.action, ContextAction::ToggleSelectionStat { .. })),
        "列头菜单不该出现选中统计开关"
    );
}

/// 只为把状态行真正渲染进窗口的探针（几何必须来自真实布局，不能手算）
struct StatusRowHarness {
    selection_stat: Option<String>,
}

impl gpui_kit::Render for StatusRowHarness {
    fn render(
        &mut self,
        _window: &mut gpui_kit::Window,
        cx: &mut gpui_kit::Context<Self>,
    ) -> impl gpui_kit::IntoElement {
        let status = ResultStatus {
            total_rows: 2,
            selected_rows: 1,
            selected_columns: 1,
            selected_cells: 1,
            ..ResultStatus::default()
        };
        render_status_row(
            Some(&status),
            ResultControls::default(),
            None,
            None,
            self.selection_stat.clone(),
            Some(12),
            None,
            None,
            Some("内存 1 KB".to_string()),
            None,
            &std::collections::BTreeSet::new(),
            None,
            None,
            cx,
        )
    }
}

/// 【选中统计 2026-11-21】状态行统计段只在有文本时出现，且排在「计算行数」钮之后、
/// 右段「内存 / 耗时 / 条数」之前（对齐 DBeaver `selectionStatLabel` 紧跟 `rowCountLabel`，
/// `ResultSetViewer.java:2182-2184`）。
///
/// 纯函数只管拼文案；这条钉的是**渲染接线**——面板传了文本、状态行却没摆（或摆到右段里
/// 去），文案再对也看不见 / 位置错。段有值 / 无值两条都在这里钉。
#[gpui_kit::test]
fn status_row_shows_the_selection_stat_segment_only_when_it_has_text(
    cx: &mut gpui_kit::TestAppContext,
) {
    cx.update(gpui_kit::init);

    // ① 有文本：段在，占一定宽度
    let (_, vcx) = cx.add_window_view(|_window, _cx| StatusRowHarness {
        selection_stat: Some("行数: 1, 列数: 1, 单元格数: 1".to_string()),
    });
    for _ in 0..2 {
        vcx.update(|window, cx| {
            window.draw(cx).clear(cx);
        });
    }
    let shown = vcx
        .debug_bounds("editor-result-selection-stat")
        .expect("面板传了统计文本，状态行却没摆这一段");
    assert!(
        shown.size.width > gpui_kit::px(0.),
        "统计段宽度为 0（被 flex 挤没了）：{:?}",
        shown
    );
    // 位置：在状态行右侧元信息（flex_1 占满 + ml_auto）**之前**，所以不能跑到最右端
    let bar = vcx
        .debug_bounds("editor-result-status-row")
        .expect("状态行应有 debug 几何");
    assert!(
        shown.left() < bar.right(),
        "统计段落在状态行之外：seg={:?} bar={:?}",
        shown,
        bar
    );

    // ② 无文本（三项全关 / 全空串）：段不存在
    let (_, vcx) = cx.add_window_view(|_window, _cx| StatusRowHarness {
        selection_stat: None,
    });
    for _ in 0..2 {
        vcx.update(|window, cx| {
            window.draw(cx).clear(cx);
        });
    }
    assert!(
        vcx.debug_bounds("editor-result-selection-stat").is_none(),
        "三项全关时状态行不该摆统计段（出厂视觉应与 2026-10-28 三项口径一致）"
    );
}

/// 【DBeaver 对齐】右键 layout 段的「呈现方式」组（`ResultSetViewer.java:3549-3578`）：
/// 网格态 = 记录 / 切换表示 / ✓ 网格 / 文本；记录开着时带 `✓ ` 前缀（DBeaver 的
/// `ToggleModeAction` 是 `AS_CHECK_BOX`）；文本态**不摆**「记录」（DBeaver 按
/// `activePresentationDescriptor.supportsRecordMode()` 门控，`:3550`），此时首项变成
/// 「切换表示」并接过组首的分隔线。
#[test]
fn layout_presentation_menu_items_follow_dbeaver_order() {
    // ① 网格态、记录关
    let grid_off = target("x", "id", 1);
    let items = layout_presentation_menu_items(&grid_off);
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
    assert_eq!(labels, vec!["记录", "切换表示", "✓ 网格", "文本"]);
    assert_eq!(items[0].action, ContextAction::ToggleRecordMode);
    assert_eq!(items[1].action, ContextAction::CyclePresentation);
    assert_eq!(
        items[2].action,
        ContextAction::SetPresentation(Presentation::Grid)
    );
    assert_eq!(
        items[3].action,
        ContextAction::SetPresentation(Presentation::Text)
    );
    // 组首带分隔线（与上方命令隔开），其余不带
    assert!(items[0].separator_before);
    assert!(!items[1].separator_before);
    assert!(!items[2].separator_before);
    assert!(!items[3].separator_before);

    // ② 记录开着：`✓ 记录`
    let mut recording = target("x", "id", 1);
    recording.record_mode = true;
    let items = layout_presentation_menu_items(&recording);
    assert_eq!(items[0].label, "✓ 记录");

    // ③ 文本态：没有「记录」，勾选落在「文本」
    let mut text = target("x", "id", 1);
    text.presentation = Presentation::Text;
    let items = layout_presentation_menu_items(&text);
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
    assert_eq!(labels, vec!["切换表示", "网格", "✓ 文本"]);
    assert!(
        items[0].separator_before,
        "文本态首项（切换表示）也要与上方命令隔开"
    );
    assert!(!items[1].separator_before);
    assert!(!items[2].separator_before);
}

/// 【DBeaver 对齐】布局段在右键里的位置：整段排在选中统计三颗**之前**
/// （`fillLayoutMenu` 里 presentation 段 `:3550-3577` 在前、统计段 `:3581-3587` 在后、
/// 筛选条显隐 `:3589-3592` 在后），而筛选条显隐是尾部最后一项；四段都排在持久化项之后。
#[test]
fn context_menu_puts_layout_group_before_selection_stats() {
    let mut probe = target_with_table("1", "id", 0);
    probe.persist = true;
    let items = context_menu_items(&probe);
    let tail: Vec<&str> = items[items.len() - 8..]
        .iter()
        .map(|i| i.label.as_str())
        .collect();
    assert_eq!(
        tail,
        vec![
            "记录",
            "切换表示",
            "✓ 网格",
            "文本",
            "显示选定的行数",
            "显示选定的列数",
            "显示选定的单元格数",
            "✓ 显示过滤器面板",
        ],
        "布局段（4 项）应紧挨在选中统计三颗之前，筛选条显隐排在最后"
    );
    // 三段各自的首项都带分隔线（DBeaver 段与段之间都有 separator）
    assert!(items[items.len() - 8].separator_before);
    assert!(items[items.len() - 4].separator_before);
    assert!(items[items.len() - 1].separator_before);
    // 持久化项在这八项之前
    let persist_at = items
        .iter()
        .position(|i| matches!(i.action, ContextAction::PersistResultSet))
        .expect("可持久化落点应摆持久化项");
    assert!(persist_at < items.len() - 8);

    // 列头菜单不摆布局段（DBeaver layout 菜单只从表格右键进）
    let column_items = column_menu_items(&probe);
    assert!(
        !column_items.iter().any(|i| matches!(
            i.action,
            ContextAction::ToggleRecordMode
                | ContextAction::CyclePresentation
                | ContextAction::SetPresentation(_)
                | ContextAction::ToggleFilterPanel { .. }
        )),
        "列头菜单不该出现呈现方式组 / 筛选条显隐"
    );
}

/// 【布局段】呈现方式循环 = 网格 ⇄ 文本（对齐 DBeaver `switchPresentation():1161-1172`
/// 取 `availablePresentations` 的下一个）；delegate 侧镜像可被右键菜单读到，
/// 且与 `selection_stat_prefs` 同口径——换结果（`set_data`）不清掉。
#[test]
fn presentation_cycles_between_grid_and_text_and_mirrors_into_delegate() {
    assert_eq!(Presentation::Grid.next(), Presentation::Text);
    assert_eq!(Presentation::Text.next(), Presentation::Grid);
    // 两档时连按两次回到原点
    assert_eq!(Presentation::Grid.next().next(), Presentation::Grid);

    let mut grid = grid_with(&[&["1", "a"]]);
    assert_eq!(
        grid.presentation_view(),
        Presentation::Grid,
        "出厂态应为网格（Presentation::default）"
    );
    grid.set_presentation_view(Presentation::Text);
    assert_eq!(grid.presentation_view(), Presentation::Text);
    // 换一批结果：呈现方式是面板视图偏好，不随数据清掉
    grid.set_data(vec!["id".into()], vec![vec!["2".into()]]);
    assert_eq!(
        grid.presentation_view(),
        Presentation::Text,
        "呈现方式镜像不该被 set_data 复位"
    );
}

/// 【DBeaver 对齐】「显示过滤器面板」项（`ResultSetViewer.fillLayoutMenu:3589-3592` 的
/// `ToggleFilterPanelAction`）：文案照中文包 `ResultSetMessages_zh.properties:131`；
/// 显示中带 `✓ ` 前缀、`open` 载荷取反（DBeaver `AS_CHECK_BOX` + `isChecked()` 读偏好）；
/// 该项恒带分隔线（DBeaver 在它前面先 `add(new Separator())`）。
#[test]
fn filter_panel_menu_item_flags_visibility_and_flips_on_click() {
    let shown = target("x", "id", 1);
    let item = filter_panel_menu_item(&shown);
    assert_eq!(item.label, "✓ 显示过滤器面板");
    assert_eq!(item.action, ContextAction::ToggleFilterPanel { open: false });
    assert!(item.separator_before, "DBeaver 在这项前面有一条 Separator");

    let mut hidden = target("x", "id", 1);
    hidden.show_filter_panel = false;
    let item = filter_panel_menu_item(&hidden);
    assert_eq!(item.label, "显示过滤器面板");
    assert_eq!(item.action, ContextAction::ToggleFilterPanel { open: true });
    assert!(item.separator_before);
}

/// 【筛选条显隐】delegate 侧镜像：出厂「显示」、`set_show_filter_panel` 可翻转、
/// 换结果（`set_data`）不清（与 `selection_stat_prefs` / `presentation_view` 同口径）。
///
/// 内部存的是**反向**布尔（`filter_panel_hidden`），因为 `ResultGridDelegate` 走
/// `derive(Default)`，而出厂语义是「显示」——这条用例同时钉住默认值落在正确一侧。
#[test]
fn delegate_mirrors_filter_panel_visibility_and_defaults_to_shown() {
    let mut grid = grid_with(&[&["1", "a"]]);
    assert!(
        grid.show_filter_panel(),
        "出厂应显示筛选条（DBeaver `DataEditorsPreferencesInitializer.java:67` 默认 true）"
    );
    grid.set_show_filter_panel(false);
    assert!(!grid.show_filter_panel());
    // 换一批结果：显隐是面板视图偏好，不随数据清掉
    grid.set_data(vec!["id".into()], vec![vec!["2".into()]]);
    assert!(
        !grid.show_filter_panel(),
        "筛选条显隐镜像不该被 set_data 复位"
    );
    grid.set_show_filter_panel(true);
    assert!(grid.show_filter_panel());
}

/// 【DBeaver 对齐】filters 子菜单不只属于列头：`fillContextMenu:3169-3177` 对
/// DATA / COLUMN_HEADER / ROW_HEADER 三个落点都挂 filtersMenu（无落点门控），所以
/// `fillFiltersMenu` 尾部四项（`:3926-3932`）在数据格右键也摆。位置照 `fillContextMenu`：
/// 复制段之后（`:3166` 先一条 Separator）、排序段之前（order 子菜单在 filters 之后，`:3179-3187`）。
#[test]
fn data_cell_context_menu_also_offers_the_filter_settings_group() {
    let target = target("orders", "name", 1);
    let items = context_menu_items(&target);

    // 四项的文案 / 动作 / 分隔线口径只在 `filter_settings_menu_items` 一处定义
    let expected = filter_settings_menu_items();
    assert_eq!(expected.len(), 4, "filters 段固定四项");
    assert!(
        expected[0].separator_before,
        "四项前有一条 Separator（DBeaver `:3166` 先 `add(new Separator())` 再挂 filtersMenu）"
    );
    assert!(
        expected[1..].iter().all(|item| !item.separator_before),
        "四项内部不再插分隔线（DBeaver 是一条 Separator 后四连摆）"
    );

    let start = items
        .iter()
        .position(|item| item.action == ContextAction::SaveFilterSetting)
        .expect("数据格右键要摆 filters 段");
    let group: Vec<&str> = items[start..start + 4]
        .iter()
        .map(|item| item.label.as_str())
        .collect();
    assert_eq!(
        group,
        vec![
            "保存过滤器设置",
            "重置过滤器设置",
            "删除所有过滤 / 排序",
            "编辑筛选设置…",
        ]
    );

    // 位置：复制段之后、排序段之前
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
        last_copy < start,
        "filters 段排在复制段之后（DBeaver `:3166-3177`）"
    );
    assert!(
        start + 3 < first_sort,
        "filters 段排在排序段之前（DBeaver order 子菜单在 filters 之后，`:3179-3187`）"
    );

    // 列头菜单仍只摆一份（两处共用同一纯函数，不允许任一处重复 extend）
    let column_items = column_menu_items(&target);
    let column_saves = column_items
        .iter()
        .filter(|item| item.action == ContextAction::SaveFilterSetting)
        .count();
    assert_eq!(column_saves, 1, "列头菜单同样只摆一份");
}

/// 【DBeaver 对齐】数据格右键的**段序**照 `ResultSetViewer.fillContextMenu:3166-3219`：
/// 复制段 → filters（`:3169-3177`）→ order（`:3179-3187`）→ edit 行级动作（`:3197-3218`）
/// → 列级视图段（冻结 / 隐藏 / 统计，`:3219` 之后）→ …（尾部仍由 layout 三段收尾）。
///
/// 本仓没有 navigate 子菜单（`:3189-3195`），也不做子菜单（扁平口径），所以只钉相对顺序。
#[test]
fn context_menu_sections_follow_fill_context_menu_order() {
    let mut editable = target("orders", "name", 1);
    editable.editable = true;
    let items = context_menu_items(&editable);
    let index = |action: ContextAction| {
        items
            .iter()
            .position(|item| item.action == action)
            .unwrap_or_else(|| panic!("菜单里没有 {action:?}"))
    };

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
    let save_filter = index(ContextAction::SaveFilterSetting);
    let first_sort = items
        .iter()
        .position(|item| matches!(item.action, ContextAction::SortDown { .. }))
        .expect("排序段至少有一项");
    let last_sort = items
        .iter()
        .rposition(|item| matches!(item.action, ContextAction::SortDown { .. }))
        .expect("排序段至少有一项");
    let insert_row = index(ContextAction::InsertRowBelow(0));
    let delete_row = index(ContextAction::DeleteRow(0));
    let freeze = index(ContextAction::ToggleFreeze(1));

    assert!(last_copy < save_filter, "filters 段在复制段之后（`:3166-3177`）");
    assert!(save_filter < first_sort, "order 段在 filters 之后（`:3179-3187`）");
    assert!(
        last_sort < insert_row && last_sort < delete_row,
        "edit 行级动作在 order 之后（`:3197-3218`）"
    );
    assert!(
        delete_row < freeze,
        "列级视图段（冻结起）在行级动作之后（`:3219` 的 Separator + `:3237` view 子菜单）"
    );

    // 分隔线：filters 首项、edit 首项、视图段首项各一条；order 内部与 filters→order 之间没有
    let save_item = &items[save_filter];
    assert!(save_item.separator_before, "filters 段前一条 Separator（`:3166`）");
    assert!(
        !items[first_sort].separator_before,
        "order 段紧跟 filters，无前导分隔线（两个子菜单相邻）"
    );
    assert!(
        items[insert_row].separator_before,
        "edit 段前一条 Separator（`:3217`）"
    );
    assert!(
        items[freeze].separator_before,
        "视图段前一条 Separator（`:3219`）"
    );
}
/// 【DBeaver 对齐】filters 整组的首项是「在结果中查找…」（`fillFiltersMenu:3869` 的
/// `EDIT_FIND_AND_REPLACE`）：命令态，动作是 `OpenFind`（只开不关）；随后是四项设置，
/// 四条设置项那一组自带 `separator_before`（`:3926` 的 Separator），所以两小组之间有线。
#[test]
fn filters_group_starts_with_the_find_command() {
    let items = filters_menu_items();
    assert_eq!(items.len(), 5, "查找 + 四项设置");
    assert_eq!(items[0].label, "在结果中查找…");
    assert_eq!(items[0].action, ContextAction::OpenFind);
    assert!(items[0].separator_before, "整段前一条分隔线（`:3166`）");
    assert_eq!(items[1].label, "保存过滤器设置");
    assert!(
        items[1].separator_before,
        "四条设置项前还有一条分隔线（`:3926` 的 Separator）"
    );
    assert!(
        items[2..].iter().all(|item| !item.separator_before),
        "四条设置项内部不再插分隔线"
    );

    // 两处落点（数据格右键 / 列头菜单）的首项都是查找命令
    let target = target("orders", "name", 1);
    let data_items = context_menu_items(&target);
    let first_filter = data_items
        .iter()
        .position(|item| item.action == ContextAction::OpenFind)
        .expect("数据格右键要摆查找命令");
    assert_eq!(data_items[first_filter].label, "在结果中查找…");
    let column_items = column_menu_items(&target);
    assert!(
        column_items
            .iter()
            .any(|item| item.action == ContextAction::OpenFind),
        "列头菜单同样要摆（DBeaver filtersMenu 对三个落点都挂）"
    );
}

/// 【D10-2】行号槽按下 = 选中整行（DBeaver `LightGrid.java:3125-3132`）。
///
/// DBeaver 的 row header 不在 `columns` 里，`getCells(row, …)` 铺出来的是全部数据列；
/// 本仓行号槽是组件第 0 列，所以 `click_cell` 把命中行号槽的焦点列归一成末列。
/// 这条把四件事一起钉住：单行全宽、Shift 点行号槽按行区间扩展、行号槽不算数据列、
/// 隐藏列后整行跟着收窄。
#[test]
fn clicking_the_row_number_slot_covers_the_whole_row() {
    // 2 数据列 → 组件列 0(#) / 1 / 2
    let mut grid = grid();

    // 无修饰键：单行 + 全宽
    grid.click_cell(0, 0, false);
    let range = grid.cell_range().expect("选区应已建立");
    assert_eq!(range.rows(), (0, 0));
    assert_eq!(range.cols(), (0, 2), "行号槽点击要铺到末列");
    assert_eq!(range.data_columns(), vec![1, 2], "行号槽不是数据列");
    assert_eq!(range.data_cells_count(), 2, "整行 = 1 行 × 2 数据列");

    // Shift 点行号槽：锚点行不动、列仍全宽（DBeaver `:3128-3129` 的整行区间）
    grid.click_cell(1, 0, true);
    let range = grid.cell_range().expect("选区应已建立");
    assert_eq!(range.rows(), (0, 1));
    assert_eq!(range.cols(), (0, 2));
    assert_eq!(range.data_cells_count(), 4);

    // 数据格点击不受这套归一影响：焦点就是该格
    grid.click_cell(0, 1, false);
    assert_eq!(grid.cell_range().unwrap().cols(), (1, 1));

    // 隐藏一列后「末列」跟着变（整行 = 现存可见列，不是历史列数）
    grid.hide_column(1);
    grid.click_cell(0, 0, false);
    let range = grid.cell_range().expect("选区应已建立");
    assert_eq!(range.cols(), (0, 1), "隐藏后只剩行号槽 + 1 个数据列");
    assert_eq!(range.data_columns(), vec![1]);
}

/// 【D10-3】左上交角 = 全选：矩形铺满全部行 × 全部列，空表不选。
///
/// DBeaver `LightGrid.java:3174-3184` 命中「行头带 ∩ 列头带」的左上格 →
/// `selectAllCellsInternal`（`:4272-4296`）：先挡空表（无列 / 无行直接 `return null`），
/// 再把 `getAllCells()` 整个塞进选区。本仓用单矩形表达同一件事：`(0,0)` 到
/// `(末行, 末列)`。列 0 是行号槽，计数由 `data_columns()` 排除，
/// 所以「2 行 × 2 数据列 × 4 格」与 DBeaver 的行头不在 `columns` 里同口径。
#[gpui_kit::test]
fn selecting_all_cells_covers_every_row_and_column() {
    let mut grid = grid(); // 2 行 × 2 数据列（id / name）

    assert!(grid.select_all_cells(), "有行有列时全选应成立");
    let range = grid.cell_range().expect("全选后应有选区");
    assert_eq!(range.rows(), (0, 1), "全部行（2 行）");
    assert_eq!(range.cols(), (0, 2), "从行号槽铺到末列");
    assert_eq!(range.data_columns(), vec![1, 2], "行号槽不是数据列");
    assert_eq!(range.data_cells_count(), 4, "2 行 × 2 数据列");

    // 「全部列」跟当前可见列走：隐藏一列后只剩 1 个数据列，格数跟着收
    grid.hide_column(1);
    assert!(grid.select_all_cells());
    let range = grid.cell_range().expect("全选后应有选区");
    assert_eq!(range.cols(), (0, 1), "隐藏后全部列 = 行号槽 + 1 个数据列");
    assert_eq!(range.data_columns(), vec![1]);
    assert_eq!(range.data_cells_count(), 2);

    // 空表两档都不选，与 DBeaver 那两条 `return null`（`:4273-4277`）一致
    let mut no_columns = ResultGridDelegate::empty("尚未执行");
    assert!(!no_columns.select_all_cells(), "没有列 → 不选");
    assert!(no_columns.cell_range().is_none());

    let mut no_rows = ResultGridDelegate::empty("尚未执行");
    no_rows.set_data(
        vec!["id".to_string()],
        Vec::<Vec<String>>::new(),
    );
    assert!(!no_rows.select_all_cells(), "没有行 → 不选");
    assert!(no_rows.cell_range().is_none());
}

/// 【D10-3】点表头左上角那一格（行号槽表头）= 全选。
///
/// 这条钉**交互链路**：真鼠标事件落到 `editor-result-th-0` 上 → delegate 的
/// `select_all_cells` 生效。组件的列头点击（`on_col_head_click`）报 `SelectColumn(0)`，
/// 面板收到会去「按行号槽排序」——所以 delegate 那边必须 `stop_propagation` 截断，
/// 这一刀在 `grid/header.rs` 的对应分支里，`HeaderHarness` 是一条真实渲染链路。
#[gpui_kit::test]
fn clicking_the_top_left_corner_selects_every_cell(cx: &mut gpui_kit::TestAppContext) {
    use gpui_kit::{Modifiers, MouseButton};

    cx.update(gpui_kit::init);
    let (harness, vcx) = cx.add_window_view(|window, cx| HeaderHarness {
        state: cx.new(|cx| gpui_kit::component::table::TableState::new(grid(), window, cx)),
    });

    // 首帧只排位置，几何第二帧才稳定
    for _ in 0..2 {
        vcx.update(|window, cx| {
            window.draw(cx).clear(cx);
        });
    }

    let corner = vcx
        .debug_bounds("editor-result-th-0")
        .expect("行号槽表头应有 debug 几何");

    // 先 move 再按下：命中测试读窗口记录的最近一次指针位置
    vcx.simulate_mouse_move(corner.center(), MouseButton::Left, Modifiers::default());
    vcx.simulate_mouse_down(corner.center(), MouseButton::Left, Modifiers::default());
    vcx.simulate_mouse_up(corner.center(), MouseButton::Left, Modifiers::default());

    let range = vcx.update(|_window, cx| {
        harness
            .read(cx)
            .state
            .read(cx)
            .delegate()
            .cell_range()
            .expect("交角点击后应有选区")
    });
    assert_eq!(range.rows(), (0, 1), "全部行");
    assert_eq!(range.cols(), (0, 2), "从行号槽铺到末列");
    assert_eq!(range.data_cells_count(), 4, "2 行 × 2 数据列");
}

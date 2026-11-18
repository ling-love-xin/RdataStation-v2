//! panel/ 全量单元测试

use super::clamp_right_sidebar_height;
use super::clamp_right_sidebar_width;
use super::next_right_edge_state;
use super::reorder_by;
use super::right_tab_from_index;
use super::sidebar_width_anim;
use super::tabs::stat_value;
use super::tabs::value_payload;
use crate::column_browser::filter_browser_columns;
use crate::grid::stats::ColumnStats;
use super::tabs::arrange_groups;
use crate::ports::{GroupSort, RightTab, StatsItem, StatsMode};
use crate::store::ColumnKind;
use gpui_kit::Pixels;
use gpui_kit::px;

/// 【保存值】字节与默认名按值形态给（纯函数；宿主只负责选路径 + 写盘）。
///
/// 四个分支各钉一条：文本 / JSON 写原文，二进制写 hex 还原的真字节（不是 hex 文本），
/// 图片扩展名随魔数（PNG → `.png`），这样存下来的图片文件能直接被看图软件打开。
#[test]
fn value_payload_extracts_bytes_and_extension_by_shape() {
    // 文本：原样字节 + .txt（行列号 1 基）
    let p = value_payload(ColumnKind::Unknown, "1234.5", 0, 1);
    assert_eq!(p.bytes, b"1234.5".to_vec());
    assert_eq!(p.default_name, "value_r1_c1.txt");

    // JSON：原文 + .json（存下来仍是合法 JSON，不是美化后的树）
    let p = value_payload(ColumnKind::Json, "{\"a\":1}", 4, 2);
    assert_eq!(p.bytes, b"{\"a\":1}".to_vec());
    assert_eq!(p.default_name, "value_r5_c2.json");

    // 二进制（非图片）：hex 还原真字节 + .bin
    let p = value_payload(ColumnKind::Binary, "0102FF", 0, 3);
    assert_eq!(p.bytes, vec![0x01, 0x02, 0xFF]);
    assert_eq!(p.default_name, "value_r1_c3.bin");

    // 图片（PNG 魔数）：真字节 + .png（扩展名来自魔数，大写格式名转小写）
    let p = value_payload(ColumnKind::Binary, "89504E470D0A1A0A", 7, 1);
    assert_eq!(
        p.bytes,
        vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]
    );
    assert_eq!(p.default_name, "value_r8_c1.png");
}

/// 三份等长样本：列名 / 类型名 / 注释（注释可空）
fn sample() -> (Vec<String>, Vec<String>, Vec<Option<String>>) {
    let names = vec![
        "id".to_string(),
        "user_name".to_string(),
        "created_at".to_string(),
    ];
    let types = vec![
        "INTEGER".to_string(),
        "VARCHAR".to_string(),
        "TIMESTAMP".to_string(),
    ];
    let comments = vec![
        Some("主键".to_string()),
        Some("用户登录名".to_string()),
        None,
    ];
    (names, types, comments)
}

/// 侧栏宽度动画参数：首帧免动画（from==to）/ 展开 0→全宽 / 收起 全宽→0
#[test]
fn sidebar_width_anim_first_frame_and_directions() {
    let full = px(36.);
    // 首帧（epoch=0）：无论开合都直接落位，不开场动画
    let (from, to) = sidebar_width_anim(0, true, full);
    assert_eq!(from, to);
    assert_eq!(to, full);
    let (from, to) = sidebar_width_anim(0, false, full);
    assert_eq!(from, to);
    assert_eq!(to, px(0.));
    // 展开（epoch>0）：0 → 全宽
    let (from, to) = sidebar_width_anim(1, true, full);
    assert_eq!(from, px(0.));
    assert_eq!(to, full);
    // 收起（epoch>0）：全宽 → 0
    let (from, to) = sidebar_width_anim(1, false, full);
    assert_eq!(from, full);
    assert_eq!(to, px(0.));
    // 类型约束（Pixels 参与计算，编译期校验）
    let _: (Pixels, Pixels) = sidebar_width_anim(2, true, full);
}

/// 【R5 可拖分栏】宽度夹取：`dx` 向右为正（右栏变窄），结果恒在 `crate::ui` 的上下限内
#[test]
fn right_sidebar_width_clamps_between_ui_limits() {
    let min = px(crate::ui::RESULT_SIDEBAR_RIGHT_MIN_WIDTH * 16.0);
    let max = px(crate::ui::RESULT_SIDEBAR_RIGHT_MAX_WIDTH * 16.0);
    let default = px(crate::ui::RESULT_SIDEBAR_RIGHT_WIDTH * 16.0);
    // 小幅向右拖（变窄）/ 向左拖（变宽）都按位移精确换算
    assert_eq!(clamp_right_sidebar_width(default, 40.0), px(280.));
    assert_eq!(clamp_right_sidebar_width(default, -40.0), px(360.));
    // 拖过头：夹在下限 / 上限
    assert_eq!(clamp_right_sidebar_width(default, 9999.0), min);
    assert_eq!(clamp_right_sidebar_width(default, -9999.0), max);
    // 已在边界上再往同向拖：保持边界，不反向越界
    assert_eq!(clamp_right_sidebar_width(min, 10.0), min);
    assert_eq!(clamp_right_sidebar_width(max, -10.0), max);
}

/// 【R5 可拖分栏】拖拽请求改宽度 → 夹取 + 关掉开合补间；点开关重新打开补间
#[gpui_kit::test]
fn resize_request_clamps_width_and_suspends_the_open_animation(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let (panel, vcx) = panel_harness(cx);
    vcx.update(|window, cx| window.draw(cx).clear(cx));
    vcx.update(|_, app| {
        panel.update(app, |p, cx| {
            // 向左拖 400px：远超上限，应夹到 640px
            p.resize_right_sidebar(-400.0, cx);
            assert_eq!(
                p.right_sidebar_width,
                px(crate::ui::RESULT_SIDEBAR_RIGHT_MAX_WIDTH * 16.0)
            );
            assert!(
                p.right_sidebar_dragging,
                "拖拽后应直写宽度（不再重播开合补间）"
            );
            // 从上限再向右拖 999px：夹到下限 192px
            p.resize_right_sidebar(999.0, cx);
            assert_eq!(
                p.right_sidebar_width,
                px(crate::ui::RESULT_SIDEBAR_RIGHT_MIN_WIDTH * 16.0)
            );
            // 点开关（三态轮转一次）：开合补间恢复
            p.toggle_value_sidebar(cx);
            assert!(
                !p.right_sidebar_dragging,
                "点开关后应恢复 160ms 开合补间"
            );
        });
    });
}

/// 【R5 可拖分栏】拖拽手柄只在**宽展开**时挂载（收起 / 窄展开没有可拖的边）
#[gpui_kit::test]
fn resize_handle_mounts_only_when_the_sidebar_is_expanded(cx: &mut gpui_kit::TestAppContext) {
    use gpui_kit::test::TestWindowExt as _;
    use gpui_kit::{ElementId, SharedString};

    cx.update(gpui_kit::init);
    let (panel, vcx) = panel_harness(cx);
    let handle_present = |vcx: &mut gpui_kit::VisualTestContext| -> bool {
        vcx.update(|window, _| {
            window
                .try_find(ElementId::Name(SharedString::from(
                    "editor-result-right-sidebar-resize",
                )))
                .is_some_and(|snap| snap.visible())
        })
    };
    // 默认宽展开（`show_value_sidebar = true`，epoch 0 免动画）：手柄挂在右栏左边缘
    vcx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(handle_present(vcx), "宽展开时右栏左边缘应有可拖手柄");
    // 收起（完全不占宽度）：手柄随右栏内容一起卸载
    vcx.update(|_, app| panel.update(app, |p, cx| p.toggle_value_sidebar(cx)));
    vcx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(!handle_present(vcx), "收起态不应有拖拽手柄");
    // 窄展开（只显右缘图标条，`show = false`）：仍无手柄
    vcx.update(|_, app| panel.update(app, |p, cx| p.toggle_value_sidebar(cx)));
    vcx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(!handle_present(vcx), "窄展开态不应有拖拽手柄");
    // 再回宽展开：先把宽度切到「拖拽直写」档，避开 160ms 补间首帧的 0 宽
    vcx.update(|_, app| {
        panel.update(app, |p, cx| {
            p.toggle_value_sidebar(cx);
            p.resize_right_sidebar(0.0, cx);
        });
    });
    vcx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(handle_present(vcx), "回到宽展开后手柄应重新挂载");
}

/// 【R5 比例持久化】偏好项注入宽度：按拖拽路径同一套上下限夹取（超界不静默放过）
#[gpui_kit::test]
fn sidebar_width_pref_is_injected_and_clamped(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let (panel, vcx) = panel_harness(cx);
    vcx.update(|_, app| {
        panel.update(app, |p, cx| {
            let min = px(crate::ui::RESULT_SIDEBAR_RIGHT_MIN_WIDTH * 16.0);
            let max = px(crate::ui::RESULT_SIDEBAR_RIGHT_MAX_WIDTH * 16.0);
            // 常态值（默认 320）原样落位
            p.set_right_sidebar_width_px(320.0, cx);
            assert_eq!(p.right_sidebar_width, px(320.));
            // 低于下限 / 高于上限：夹到边界（复用 `clamp_right_sidebar_width`）
            p.set_right_sidebar_width_px(100.0, cx);
            assert_eq!(p.right_sidebar_width, min);
            p.set_right_sidebar_width_px(9999.0, cx);
            assert_eq!(p.right_sidebar_width, max);
        });
    });
}

/// 【R5 比例持久化】松手才交宽度：非拖拽态返回 `None`（点了下手柄不该触发写盘）
#[gpui_kit::test]
fn end_resize_reports_width_only_while_dragging(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let (panel, vcx) = panel_harness(cx);
    vcx.update(|_, app| {
        panel.update(app, |p, cx| {
            // 初始非拖拽：不交宽度
            assert_eq!(p.end_right_sidebar_resize(cx), None);
            // 向右拖 40px（默认 320 → 280），松手回传夹取后的宽度并清标记
            p.resize_right_sidebar(40.0, cx);
            assert_eq!(p.end_right_sidebar_resize(cx), Some(px(280.)));
            assert!(!p.right_sidebar_dragging, "松手后应清拖拽标记");
            // 再松一次：已是非拖拽态，不重复交宽度
            assert_eq!(p.end_right_sidebar_resize(cx), None);
        });
    });
}

/// 【R5 布局方向切换】竖排高度夹取：与宽度同一套「朝手柄方向即变小」口径
#[test]
fn right_sidebar_height_clamps_between_ui_limits() {
    let min = px(crate::ui::RESULT_SIDEBAR_RIGHT_MIN_HEIGHT * 16.0);
    let max = px(crate::ui::RESULT_SIDEBAR_RIGHT_MAX_HEIGHT * 16.0);
    let default = px(crate::ui::RESULT_SIDEBAR_RIGHT_HEIGHT * 16.0);
    // 顶边手柄：向下拖（dy 正）→ 变矮；向上拖 → 变高
    assert_eq!(clamp_right_sidebar_height(default, 40.0), px(200.));
    assert_eq!(clamp_right_sidebar_height(default, -40.0), px(280.));
    // 拖过头：夹在上下限，不反向越界
    assert_eq!(clamp_right_sidebar_height(default, 9999.0), min);
    assert_eq!(clamp_right_sidebar_height(default, -9999.0), max);
    assert_eq!(clamp_right_sidebar_height(min, 10.0), min);
    assert_eq!(clamp_right_sidebar_height(max, -10.0), max);
}

/// 【R5 布局方向切换】切方向翻状态并清拖拽标记；竖排拖拽改高度、松手不回传（高度不落盘）
#[gpui_kit::test]
fn toggle_vertical_layout_flips_and_stops_persisting_widths(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let (panel, vcx) = panel_harness(cx);
    vcx.update(|_, app| {
        panel.update(app, |p, cx| {
            assert!(!p.vertical_layout, "默认横排（网格左 / 右栏右）");
            p.toggle_vertical_layout(cx);
            assert!(p.vertical_layout, "切一次应变成竖排");
            assert!(!p.right_sidebar_dragging, "切方向应清掉拖拽标记");
            // 竖排拖拽：只改高度，宽度原封不动
            let width_before = p.right_sidebar_width;
            let height_before = p.right_sidebar_height;
            p.resize_right_sidebar(40.0, cx);
            assert_eq!(p.right_sidebar_width, width_before, "竖排不该改宽度");
            assert_eq!(p.right_sidebar_height, height_before - px(40.));
            // 松手：竖排不回传尺寸（高度没有对应偏好项），只清标记
            assert_eq!(p.end_right_sidebar_resize(cx), None);
            assert!(!p.right_sidebar_dragging);
            assert_eq!(
                p.right_sidebar_height,
                height_before - px(40.),
                "高度留在内存里，切回横排再切竖排仍生效"
            );
            // 切回横排：宽度原封不动（两个方向各自记自己的尺寸）
            p.toggle_vertical_layout(cx);
            assert!(!p.vertical_layout);
            assert_eq!(p.right_sidebar_width, width_before);
        });
    });
}

/// 右侧开关三态循环：收起 → 窄展开 → 宽展开 → 收起；每次点击恰好在三态间轮转
#[test]
fn right_edge_state_cycles_through_three_states() {
    // 从宽展开（初始默认）出发，点击三次回原态
    assert_eq!(next_right_edge_state(true, true), (false, false));
    assert_eq!(next_right_edge_state(false, false), (true, false));
    assert_eq!(next_right_edge_state(true, false), (true, true));
    // 宽展开恒有竖条可见（不变量）
    let (edge, show) = next_right_edge_state(true, false);
    assert!(show);
    assert!(edge);
    // 收起态点击直接进窄展开，不跨到宽展开（比左侧多一个展开态：窄 = 只显图标条）
    let (edge, show) = next_right_edge_state(false, false);
    assert!(edge);
    assert!(!show);
}

/// 空词 / 纯空白 = 全量原序（隐藏列由渲染层置灰，过滤本身不剔列）
#[test]
fn browser_filter_empty_returns_all_in_order() {
    let (n, t, c) = sample();
    assert_eq!(filter_browser_columns(&n, &t, &c, ""), vec![0, 1, 2]);
    assert_eq!(filter_browser_columns(&n, &t, &c, "   "), vec![0, 1, 2]);
}

/// 列名匹配忽略大小写；子串包含即命中
#[test]
fn browser_filter_matches_name_case_insensitive() {
    let (n, t, c) = sample();
    assert_eq!(filter_browser_columns(&n, &t, &c, "USER"), vec![1]);
    assert_eq!(filter_browser_columns(&n, &t, &c, "name"), vec![1]);
}

/// 类型名同样参与匹配（DBeaver 元数据面板只匹配列名；本轮拍板加宽到类型名 / 注释）
#[test]
fn browser_filter_matches_type_name() {
    let (n, t, c) = sample();
    assert_eq!(filter_browser_columns(&n, &t, &c, "varchar"), vec![1]);
    assert_eq!(filter_browser_columns(&n, &t, &c, "timestamp"), vec![2]);
}

/// 注释参与匹配；注释为空的列不参与注释分支
#[test]
fn browser_filter_matches_comment() {
    let (n, t, c) = sample();
    assert_eq!(filter_browser_columns(&n, &t, &c, "登录名"), vec![1]);
    assert_eq!(filter_browser_columns(&n, &t, &c, "主键"), vec![0]);
}

/// 列名 / 类型 / 注释都不命中 = 空清单（渲染层摆「无匹配列」空态）
#[test]
fn browser_filter_no_match_returns_empty() {
    let (n, t, c) = sample();
    assert!(filter_browser_columns(&n, &t, &c, "zzz").is_empty());
}

/// 页签顺序 / 下标映射（`events::right_tabs` 单一来源）：
/// 0=列浏览器（默认）/ 1=值详情 / 2=计算 / 3=分组 / 4=引用 / 5=执行SQL，越界回落默认页；
/// 反查（枚举 → 下标）与之互逆，页签条 `selected_index` 与点击回调用的是同一份顺序。
#[test]
fn right_tab_index_maps_to_the_six_panel_tabs() {
    use super::events::{right_tab_index, right_tabs};

    assert_eq!(right_tab_from_index(0), RightTab::Browser);
    assert_eq!(right_tab_from_index(1), RightTab::Value);
    assert_eq!(right_tab_from_index(2), RightTab::Stats);
    assert_eq!(right_tab_from_index(3), RightTab::Group);
    assert_eq!(right_tab_from_index(4), RightTab::Reference);
    assert_eq!(right_tab_from_index(5), RightTab::ExecSql);
    assert_eq!(right_tab_from_index(99), RightTab::Browser);

    // 互逆：每个页签既是自己的下标对应的枚举，下标也唯一
    for (index, tab) in right_tabs().into_iter().enumerate() {
        assert_eq!(right_tab_from_index(index), tab);
        assert_eq!(right_tab_index(tab), index);
    }
}

/// 【R2 计算页】单项取值：口径决定 Min/Max 的比较方式，Avg/Sum 只在数值口径出值，
/// 计数类不受口径影响（对齐 DBeaver `aggregateAsStrings`）
#[test]
fn stat_value_respects_mode_and_missing_samples() {
    let stats = ColumnStats {
        count: 1234,
        nulls: 2,
        min: Some(1.0),
        max: Some(2000.0),
        sum: Some(2001.0),
        avg: Some(1000.5),
    };
    let text = (Some("apple".to_string()), Some("zebra".to_string()));
    // 数值口径：极值走数值比较 + 千分位
    assert_eq!(stat_value(StatsItem::Min, &stats, None, StatsMode::Numeric), "1");
    assert_eq!(
        stat_value(StatsItem::Max, &stats, None, StatsMode::Numeric),
        "2,000"
    );
    // 字符串口径：极值走字典序，Avg/Sum 不出值
    assert_eq!(
        stat_value(StatsItem::Min, &stats, Some(&text), StatsMode::Text),
        "apple"
    );
    assert_eq!(
        stat_value(StatsItem::Max, &stats, Some(&text), StatsMode::Text),
        "zebra"
    );
    assert_eq!(
        stat_value(StatsItem::Avg, &stats, Some(&text), StatsMode::Text),
        "—"
    );
    assert_eq!(
        stat_value(StatsItem::Sum, &stats, Some(&text), StatsMode::Text),
        "—"
    );
    // 计数类不受口径影响（字符串口径下仍是千分位整数）
    assert_eq!(
        stat_value(StatsItem::Count, &stats, None, StatsMode::Text),
        "1,234"
    );
}

/// 【R2 计算页】默认函数集 = 六项全集、展示顺序固定；口径默认数值且可来回切
#[test]
fn stats_defaults_are_full_set_in_display_order() {
    assert_eq!(StatsItem::ALL.len(), 6);
    assert_eq!(StatsItem::ALL[0], StatsItem::Count);
    assert_eq!(StatsItem::ALL[5], StatsItem::Sum);
    // 只有 Min/Max 支持字符串口径（SUM/AVG 对字符串累计不成立）
    assert!(StatsItem::Min.supports_text_mode());
    assert!(!StatsItem::Sum.supports_text_mode());
    assert_eq!(StatsMode::default(), StatsMode::Numeric);
    assert_eq!(StatsMode::Numeric.toggled(), StatsMode::Text);
    assert_eq!(StatsMode::Text.toggled(), StatsMode::Numeric);
}

/// 【结果集 #296】空列序 = 默认序原样直通（列 / 行一字不差）
#[test]
fn empty_order_passes_columns_through_unchanged() {
    let columns = vec!["a".to_string(), "b".to_string(), "c".to_string()];
    let rows = vec![
        vec!["1".to_string(), "2".to_string(), "3".to_string()],
        vec!["4".to_string(), "5".to_string(), "6".to_string()],
    ];
    let (out_columns, out_rows) = reorder_by(&[], columns.clone(), rows.clone());
    assert_eq!(out_columns, columns);
    assert_eq!(out_rows, rows);
}

/// 【结果集 #296】按持久化列名序重排：列序与行内容同步换位
#[test]
fn reorder_moves_columns_and_rows_together() {
    let order = vec!["b".to_string(), "a".to_string()];
    let columns = vec!["a".to_string(), "b".to_string(), "c".to_string()];
    let rows = vec![
        vec!["1".to_string(), "2".to_string(), "3".to_string()],
        vec!["4".to_string(), "5".to_string(), "6".to_string()],
    ];
    let (out_columns, out_rows) = reorder_by(&order, columns, rows);
    assert_eq!(
        out_columns,
        vec!["b".to_string(), "a".to_string(), "c".to_string()],
        "持久化序在前，未匹配列（c）按数据序附加末尾"
    );
    assert_eq!(
        out_rows,
        vec![
            vec!["2".to_string(), "1".to_string(), "3".to_string()],
            vec!["5".to_string(), "4".to_string(), "6".to_string()],
        ],
        "行按列同序换位"
    );
}

/// 【结果集 #296】持久化序里的列已不在新结果里 → 只匹配存在的；全不匹配 = 数据序
#[test]
fn reorder_keeps_only_matching_columns() {
    let order = vec!["gone".to_string(), "x".to_string()];
    let columns = vec!["a".to_string(), "b".to_string()];
    let rows = vec![vec!["1".to_string(), "2".to_string()]];
    let (out_columns, out_rows) = reorder_by(&order, columns, rows);
    assert_eq!(out_columns, vec!["a".to_string(), "b".to_string()]);
    assert_eq!(out_rows, vec![vec!["1".to_string(), "2".to_string()]]);
}

/// 【2026-10-29】结果集 UI 渲染断言（gpui-kit 测试框架 `TestAppContext` /
/// `TestWindowExt` / `VisualTestContext::debug_bounds`）：渲染**真实面板**（非纯函数），
/// 断言本轮微调的关键 UI 元素存在且可见——**防回退**：
/// - 空面板根 `editor-result-root` 恒渲染；
/// - 展开筛选条后：`editor-result-filter-expand-bar` + **两内嵌按钮**
///   （左=展开 `-toggle` / 右=运行 `-run`，无边框输入框）；
/// - SQL 预览弹窗：`open_sql_dialog` 后遮罩 `editor-result-sql-dialog` 出现、
///   `close_sql_dialog` 后消失。
///
/// 查询机制（gpui-kit 0.8.1 实测）：
/// - `TestWindowExt.find/try_find` 只匹配**显式 ElementId** 元素
///   （`Button::new(id)` / `.id()` / EffectTransition 包装）——纯 `div().debug_selector()`
///   的 div **不注册**（`Observed::new` 要求 `Element::id().is_some()`）；
/// - `div().debug_selector()` 走 gpui 原生 `debug_bounds` 注册路径
///   （`VisualTestContext::debug_bounds(&'static str)`，paint 阶段注册）；
/// - 故 `visible()` 兜底查询两路：显式 id 走 `try_find().visible()`，
///   debug_selector div 走 `debug_bounds().is_some()`。
///
/// 谁删了弹窗 / 改回旧展开条 / 拆掉两内嵌按钮，此测试即红。
#[gpui_kit::test]
fn rendered_ui_elements_survive(cx: &mut gpui_kit::TestAppContext) {
    use gpui_kit::component::Root;
    use gpui_kit::test::TestWindowExt as _;
    use gpui_kit::{AppContext as _, ElementId, Entity, IntoElement, Render, SharedString};

    cx.update(gpui_kit::init);
    let slot: std::rc::Rc<std::cell::RefCell<Option<Entity<super::ResultPanel>>>> =
        std::rc::Rc::new(std::cell::RefCell::new(None));
    let slot_in = slot.clone();
    // 宿主壳：ResultPanel 无标准 `Render`（自定义 `render(&self, cx)`，host 手动调），
    // 测试仿 host.rs 9075（`result_panel.read(cx).render(cx)`）包一层 Render 实体
    struct PanelHost {
        panel: Entity<super::ResultPanel>,
    }
    impl Render for PanelHost {
        fn render(
            &mut self,
            _window: &mut gpui_kit::Window,
            cx: &mut gpui_kit::Context<Self>,
        ) -> impl IntoElement {
            self.panel.read(cx).render(cx)
        }
    }
    let (_, cx) = cx.add_window_view(move |window, cx| {
        let queue = crate::ports::ResultActionQueue::new();
        let panel = cx.new(|cx| super::ResultPanel::new(window, cx, queue));
        *slot_in.borrow_mut() = Some(panel.clone());
        let host = cx.new(|_| PanelHost { panel });
        Root::new(host, window, cx)
    });
    let panel = slot.borrow().clone().expect("ResultPanel 已创建");
    cx.update(|window, cx| window.draw(cx).clear(cx));

    let visible = |id: &'static str, cx: &mut gpui_kit::VisualTestContext| -> bool {
        let by_id = cx.update(|window, _| {
            window
                .try_find(ElementId::Name(SharedString::from(id.to_string())))
                .is_some_and(|snap| snap.visible())
        });
        // 无显式 ElementId 的 div（纯 debug_selector）走 gpui debug_bounds 注册路径
        by_id || cx.debug_bounds(id).is_some()
    };

    // 1) 空面板根恒渲染
    assert!(visible("editor-result-root", cx), "空面板根应渲染可见");

    // 1.5) 筛选区外框（DBeaver `CompositeBorderPainter` 等价物）：
    //      高 24px = `Input.xsmall()` 20 + 上下各 1px 边框 + 上下各 1px 内距；
    //      落在 30px 工具栏里居中 → 上下各 3px，正好等于 DBeaver 的 `marginHeight = 3`
    //      （`ResultSetFilterPanel.java:164`），其内部 `2 + 20 + 2` 也是 24px（`:103/:175`）。
    //      量绝对像素而不是比值：Input 档位、边框、内距、工具栏高任一改动都会先红。
    let filter_bounds = cx
        .debug_bounds("editor-result-filter")
        .expect("筛选区外框必须带 debug_selector 才量得到");
    let filter_height = f32::from(filter_bounds.size.height);
    assert!(
        (filter_height - 24.0).abs() < 0.5,
        "筛选区外框应为 24px（Input.xsmall 20 + 边框 1 与内距 1 各两道，对齐 DBeaver 的 2+20+2），实际 {filter_height}px"
    );

    // 1.6) 【筛选历史】历史下拉的**渲染接线**：空历史不摆入口，提交一条后出现
    //      （状态侧两条用例在 `commit_on_enter_records_filter_history` /
    //      `filter_history_dedupes_caps_and_replays`；这里只钉「真的画出来」）
    assert!(
        !visible("editor-result-filter-history", cx),
        "空历史不该摆历史下拉入口（能力没有就不摆）"
    );
    cx.update(|_, app| {
        panel.update(app, |p, cx| {
            p.filter_text = "1".to_string();
            p.note_filter_commit();
            // 手动改了面板状态要自己 notify：宿主壳只在实体变脏时重渲染，
            // 不 notify 的话 `window.draw` 会复用上一帧的树（探针会读到旧帧）
            cx.notify();
        })
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(
        visible("editor-result-filter-history", cx),
        "有历史后应出现历史下拉按钮"
    );

    // 2) 展开筛选条：两列按钮（左=收起 / 右=运行）+ 多行输入框随条出现
    cx.update(|_, app| panel.update(app, |p, cx| p.toggle_filter_expand(cx)));
    cx.update(|window, cx| window.draw(cx).clear(cx));
    for id in [
        "editor-result-filter-expand-bar",
        "editor-result-filter-expand-toggle",
        "editor-result-filter-expand-run",
    ] {
        assert!(
            visible(id, cx),
            "展开筛选条元素 {id} 应渲染可见（回退检查）"
        );
    }

    // 2.5) 展开条高度 = 5 行（P3 把筛选框换成多行 `Textarea` 的落点）。
    //      DBeaver 展开是 `gd.heightHint = getLineHeight() * 5`
    //      （`togglePanelExpand`，`ResultSetFilterPanel.java:1320-1327`）；本仓行高常量
    //      20px（`input.rs:543` 的 `Rems(1.25)`）→ 控件 100px + 容器 `py_1` 上下 8px
    //      + 下边框 1px = 109px。给区间不给精确值：只钉「真的变成 5 行」——
    //      若这里量到 20~30px，说明多行控件的高度没生效（那是静默的形态回退）。
    let expand_bar = cx
        .debug_bounds("editor-result-filter-expand-bar")
        .expect("展开条应带 debug_selector 才量得到");
    let expand_h = f32::from(expand_bar.size.height);
    assert!(
        (100.0..=115.0).contains(&expand_h),
        "展开条应容纳 5 行（≈109px），实际 {expand_h}px"
    );

    // 3) SQL 预览弹窗：open 出现 / close 消失
    cx.update(|window, app| {
        panel.update(app, |p, cx| p.open_sql_dialog(window, cx));
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(
        visible("editor-result-sql-dialog", cx),
        "SQL 弹窗应渲染（回退检查）"
    );
    cx.update(|_, app| panel.update(app, |p, cx| p.close_sql_dialog(cx)));
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(
        !visible("editor-result-sql-dialog", cx),
        "关闭后 SQL 弹窗应消失"
    );
}

/// 【R2】计算页真渲染（不只跑纯函数）：面板工具条三件 / 两列表头 / 六行统计（含行尾 ×）
/// 都要真画出来；行高 20px、工具条 28px；停用一项后该行消失、Reset 后回来（行为闭环）。
///
/// 谁把工具条拿掉、把 `stats_items` 写死回六行、或让 `Text` 口径把 Avg/Sum 也算出值，
/// 此用例即红。
#[gpui_kit::test]
fn stats_tab_renders_toolbar_header_and_rows(cx: &mut gpui_kit::TestAppContext) {
    use gpui_kit::component::table::TableState;
    use gpui_kit::component::Root;
    use gpui_kit::test::TestWindowExt as _;
    use gpui_kit::{AppContext as _, ElementId, Entity, IntoElement, Render, SharedString};

    // 宿主壳（与 `rendered_ui_elements_survive` 同口径：ResultPanel 自定义 render）
    struct PanelHost {
        panel: Entity<super::ResultPanel>,
    }
    impl Render for PanelHost {
        fn render(
            &mut self,
            _window: &mut gpui_kit::Window,
            cx: &mut gpui_kit::Context<Self>,
        ) -> impl IntoElement {
            self.panel.read(cx).render(cx)
        }
    }

    cx.update(gpui_kit::init);
    let slot: std::rc::Rc<std::cell::RefCell<Option<Entity<super::ResultPanel>>>> =
        std::rc::Rc::new(std::cell::RefCell::new(None));
    let slot_in = slot.clone();
    let (_, cx) = cx.add_window_view(move |window, cx| {
        let queue = crate::ports::ResultActionQueue::new();
        let panel = cx.new(|cx| super::ResultPanel::new(window, cx, queue));
        let mut delegate = crate::grid::ResultGridDelegate::default();
        delegate.set_data(
            vec!["id".to_string(), "amount".to_string()],
            vec![
                vec!["1".to_string(), "1234.5".to_string()],
                vec!["2".to_string(), "NULL".to_string()],
            ],
        );
        let grid: Entity<TableState<crate::grid::ResultGridDelegate>> =
            cx.new(|cx| TableState::new(delegate, window, cx));
        panel.update(cx, |p, cx| {
            p.set_grids(grid, None, window, cx);
            // 列浏览器清单选中第 2 列（amount）→ 计算页作用列
            p.selected_col = Some(1);
            p.set_right_tab(RightTab::Stats, cx);
        });
        *slot_in.borrow_mut() = Some(panel.clone());
        let host = cx.new(|_| PanelHost { panel });
        Root::new(host, window, cx)
    });
    let panel = slot.borrow().clone().expect("ResultPanel 已创建");
    cx.update(|window, cx| window.draw(cx).clear(cx));

    let visible = |id: &'static str, cx: &mut gpui_kit::VisualTestContext| -> bool {
        let by_id = cx.update(|window, _| {
            window
                .try_find(ElementId::Name(SharedString::from(id.to_string())))
                .is_some_and(|snap| snap.visible())
        });
        by_id || cx.debug_bounds(id).is_some()
    };

    // 1) 工具条三件 + 表头 + 六行 + 行尾 × 全部真渲染
    for id in [
        "editor-result-stats-tab",
        "editor-result-stats-toolbar",
        "editor-result-stats-header",
        "editor-result-stat-add",
        "editor-result-stat-reset",
        "editor-result-stat-mode",
        "editor-result-stat-row-Count",
        "editor-result-stat-row-Nulls",
        "editor-result-stat-row-Min",
        "editor-result-stat-row-Max",
        "editor-result-stat-row-Avg",
        "editor-result-stat-row-Sum",
        "editor-result-stat-remove-Sum",
    ] {
        assert!(visible(id, cx), "计算页元素 {id} 应渲染可见");
    }

    // 2) 量像素：工具条 28px（= vendor Toolbar XSmall 的 `min_h_7`，容器同高避免溢出）、
    //    表头与统计行各 20px（DBeaver 侧面板的 SWT 行高档）
    let toolbar_h = f32::from(
        cx.debug_bounds("editor-result-stats-toolbar")
            .expect("工具条应带 debug_selector 才量得到")
            .size
            .height,
    );
    assert!(
        (toolbar_h - 28.0).abs() < 0.5,
        "面板工具条应 28px（与 Toolbar XSmall 托底同高），实际 {toolbar_h}px"
    );
    let header_h = f32::from(
        cx.debug_bounds("editor-result-stats-header")
            .expect("表头应带 debug_selector 才量得到")
            .size
            .height,
    );
    assert!(
        (header_h - 20.0).abs() < 0.5,
        "表头应 20px，实际 {header_h}px"
    );
    let row_h = f32::from(
        cx.debug_bounds("editor-result-stat-row-Count")
            .expect("统计行应带 debug_selector 才量得到")
            .size
            .height,
    );
    assert!(
        (row_h - 20.0).abs() < 0.5,
        "统计行应 20px，实际 {row_h}px"
    );

    // 3) 行为闭环：停用 Sum → 该行消失；Reset → 回来
    cx.update(|_, app| {
        panel.update(app, |p, cx| {
            p.remove_stats_item(StatsItem::Sum, cx);
        })
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(
        !visible("editor-result-stat-row-Sum", cx),
        "停用 Sum 后该行应消失"
    );
    cx.update(|_, app| panel.update(app, |p, cx| p.reset_stats_items(cx)));
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(
        visible("editor-result-stat-row-Sum", cx),
        "Reset 后 Sum 行应回来"
    );
}

/// 【R3】分组排序 / 「仅重复」的纯函数口径（不依赖渲染）
#[test]
fn arrange_groups_sorts_and_filters() {
    let mut groups = vec![
        (vec!["b".to_string()], 1usize),
        (vec!["a".to_string()], 3),
        (vec!["c".to_string()], 1),
    ];
    // 按值序
    arrange_groups(&mut groups, GroupSort::Value, false);
    assert_eq!(
        groups.iter().map(|(key, _)| key[0].as_str()).collect::<Vec<_>>(),
        ["a", "b", "c"]
    );

    // 按计数降序：热点在前，同数按值序兜底（b 在 c 前）
    arrange_groups(&mut groups, GroupSort::CountDesc, false);
    assert_eq!(groups[0], (vec!["a".to_string()], 3));
    assert_eq!(groups[1], (vec!["b".to_string()], 1));
    assert_eq!(groups[2], (vec!["c".to_string()], 1));

    // 仅重复：只留行数 > 1 的组
    arrange_groups(&mut groups, GroupSort::CountDesc, true);
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0], (vec!["a".to_string()], 3));

    // 多列键按元组字典序比较（先比第一列，再比第二列）
    let mut multi = vec![
        (vec!["a".to_string(), "2".to_string()], 1usize),
        (vec!["a".to_string(), "1".to_string()], 2),
    ];
    arrange_groups(&mut multi, GroupSort::Value, false);
    assert_eq!(multi[0].0, vec!["a".to_string(), "1".to_string()]);
}

/// 【R3】清空分组列：对齐 DBeaver `ClearGroupingAction`（分组列非空才可用；
/// 清空后回到「选中列兜底」而不是空态）。这里钉状态与幂等，按钮存在性由
/// 渲染用例断言。
#[gpui_kit::test]
fn clear_group_columns_drops_explicit_selection(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let (panel, cx) = panel_harness(cx);

    cx.update(|_, app| {
        panel.update(app, |p, cx| {
            p.toggle_group_column(0, cx);
            p.toggle_group_column(2, cx);
        })
    });
    assert_eq!(
        cx.update(|_, app| panel.read(app).group_columns.clone()),
        vec![0, 2],
        "两次勾选应按顺序累计显式分组列"
    );

    cx.update(|_, app| panel.update(app, |p, cx| p.clear_group_columns(cx)));
    assert!(
        cx.update(|_, app| panel.read(app).group_columns.clone())
            .is_empty(),
        "清空后显式分组列应归零（分组页回落选中列兜底）"
    );

    // 幂等：空集合再清空不改变状态（对应 DBeaver 动作在空集合时不可用）
    cx.update(|_, app| panel.update(app, |p, cx| p.clear_group_columns(cx)));
    assert!(cx.update(|_, app| panel.read(app).group_columns.clone()).is_empty());
}

/// 【R1/R2 测试壳】空面板 + 两列网格（`id` / `amount`，含一个 NULL 值）。
/// `ResultPanel` 没有标准 `Render`（自定义 `render(&self, cx)`，宿主手动调），
/// 所以包一层 `PanelHost` 实体转发；`selected_col` 预置第 2 列，计算 / 分组页才有作用列。
fn panel_harness<'a>(
    cx: &'a mut gpui_kit::TestAppContext,
) -> (
    gpui_kit::Entity<super::ResultPanel>,
    &'a mut gpui_kit::VisualTestContext,
) {
    use gpui_kit::component::table::TableState;
    use gpui_kit::component::Root;
    use gpui_kit::{AppContext as _, Entity, IntoElement, Render};

    struct PanelHost {
        panel: Entity<super::ResultPanel>,
    }
    impl Render for PanelHost {
        fn render(
            &mut self,
            _window: &mut gpui_kit::Window,
            cx: &mut gpui_kit::Context<Self>,
        ) -> impl IntoElement {
            self.panel.read(cx).render(cx)
        }
    }

    let slot: std::rc::Rc<std::cell::RefCell<Option<Entity<super::ResultPanel>>>> =
        std::rc::Rc::new(std::cell::RefCell::new(None));
    let slot_in = slot.clone();
    let (_, vcx) = cx.add_window_view(move |window, cx| {
        let queue = crate::ports::ResultActionQueue::new();
        let panel = cx.new(|cx| super::ResultPanel::new(window, cx, queue));
        let mut delegate = crate::grid::ResultGridDelegate::default();
        delegate.set_data(
            vec!["id".to_string(), "amount".to_string()],
            vec![
                vec!["1".to_string(), "1234.5".to_string()],
                vec!["2".to_string(), "NULL".to_string()],
            ],
        );
        let grid: Entity<TableState<crate::grid::ResultGridDelegate>> =
            cx.new(|cx| TableState::new(delegate, window, cx));
        panel.update(cx, |p, cx| {
            p.set_grids(grid, None, window, cx);
            p.selected_col = Some(1);
        });
        *slot_in.borrow_mut() = Some(panel.clone());
        let host = cx.new(|_| PanelHost { panel });
        Root::new(host, window, cx)
    });
    let panel = slot.borrow().clone().expect("ResultPanel 已创建");
    (panel, vcx)
}

/// 【R1】右栏页内工具条形态统一：值详情 / 分组 / 执行SQL 三页各一条 28px 工具条，
/// 动作只挂已有能力（值详情=复制当前值 + 保存至文件（对齐 DBeaver
/// `ContentValueManager.contributeContentActions`）、执行SQL=复制 SQL、分组无动作——DBeaver 的
/// `ReferencesPanel.contributeActions` 也是空实现，故引用页维持空态）。
#[gpui_kit::test]
fn right_panel_pages_share_the_toolbar_shape(cx: &mut gpui_kit::TestAppContext) {
    use gpui_kit::test::TestWindowExt as _;
    use gpui_kit::{ElementId, SharedString};

    cx.update(gpui_kit::init);
    let (panel, cx) = panel_harness(cx);
    cx.update(|window, cx| window.draw(cx).clear(cx));

    let visible = |id: &'static str, cx: &mut gpui_kit::VisualTestContext| -> bool {
        let by_id = cx.update(|window, _| {
            window
                .try_find(ElementId::Name(SharedString::from(id.to_string())))
                .is_some_and(|snap| snap.visible())
        });
        by_id || cx.debug_bounds(id).is_some()
    };
    // 页内工具条统一高度 28px（`ui::RESULT_PANEL_TOOLBAR_HEIGHT`）
    let toolbar_height = |id: &'static str, cx: &mut gpui_kit::VisualTestContext| -> f32 {
        f32::from(
            cx.debug_bounds(id)
                .expect("页内工具条应带 debug_selector 才量得到")
                .size
                .height,
        )
    };

    // 0) 列浏览器（默认页）：此前手写过 24px 无分隔线的标题行，与其它页差 4px
    assert!(
        visible("editor-result-browser-toolbar", cx),
        "列浏览器页应有页内工具条（editor-result-browser-toolbar）"
    );
    let browser_toolbar_h = toolbar_height("editor-result-browser-toolbar", cx);
    assert!(
        (browser_toolbar_h - 28.0).abs() < 0.5,
        "列浏览器工具条应与其它页同高 28px，实际 {browser_toolbar_h}px"
    );

    // 1) 值详情：工具条 + 复制当前值 + 保存值（保存值对齐 DBeaver「保存至文件…」）
    cx.update(|_, app| {
        panel.update(app, |p, cx| {
            p.selection_pos = Some((0, 1));
            p.set_right_tab(RightTab::Value, cx);
        })
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    for id in [
        "editor-result-value-toolbar",
        "editor-result-value-copy",
        "editor-result-value-save",
    ] {
        assert!(visible(id, cx), "值详情页元素 {id} 应渲染可见");
    }
    let value_toolbar_h = f32::from(
        cx.debug_bounds("editor-result-value-toolbar")
            .expect("值详情工具条应带 debug_selector 才量得到")
            .size
            .height,
    );
    assert!(
        (value_toolbar_h - 28.0).abs() < 0.5,
        "值详情工具条应与计算页同高 28px，实际 {value_toolbar_h}px"
    );

    // 2) 分组：工具条 + 四个动作（列下拉 / 排序 / 仅重复 / 清空分组列）
    cx.update(|_, app| {
        panel.update(app, |p, cx| p.set_right_tab(RightTab::Group, cx))
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    for id in [
        "editor-result-group-toolbar",
        "editor-result-group-columns",
        "editor-result-group-sort",
        "editor-result-group-dup",
        "editor-result-group-clear",
    ] {
        assert!(visible(id, cx), "分组页元素 {id} 应渲染可见");
    }

    // 3) 执行SQL：工具条 + 复制
    cx.update(|_, app| {
        panel.update(app, |p, cx| {
            p.exec_sql = Some("select 1 as n".to_string());
            p.set_right_tab(RightTab::ExecSql, cx);
        })
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    for id in [
        "editor-result-exec-sql-toolbar",
        "editor-result-exec-sql-copy",
    ] {
        assert!(visible(id, cx), "执行SQL页元素 {id} 应渲染可见");
    }

    // 4) 引用：此前整页只有空态、没有工具条 → 切到本页正文会顶到页签条下方
    cx.update(|_, app| {
        panel.update(app, |p, cx| p.set_right_tab(RightTab::Reference, cx))
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(
        visible("editor-result-reference-toolbar", cx),
        "引用页应有页内工具条（editor-result-reference-toolbar）"
    );
    let reference_toolbar_h = toolbar_height("editor-result-reference-toolbar", cx);
    assert!(
        (reference_toolbar_h - 28.0).abs() < 0.5,
        "引用页工具条应与其它页同高 28px，实际 {reference_toolbar_h}px"
    );

    // 5) 空态也保留工具条：六页同形（此前空态直接返回居中文字，比其它页高 28px）
    cx.update(|_, app| {
        panel.update(app, |p, cx| {
            p.selection_pos = None;
            p.selected_col = None;
            p.set_right_tab(RightTab::Value, cx);
        })
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(
        visible("editor-result-value-toolbar", cx),
        "值详情空态（未选中单元格）也应保留工具条"
    );
    cx.update(|_, app| {
        panel.update(app, |p, cx| p.set_right_tab(RightTab::Stats, cx))
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(
        visible("editor-result-stats-toolbar", cx),
        "计算页空态（未选中列）也应保留工具条"
    );

    // 6) 分组页空态：工具条常驻（分组列下拉是唯一的选列入口，不能随空态消失）
    cx.update(|_, app| {
        panel.update(app, |p, cx| p.set_right_tab(RightTab::Group, cx))
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(
        visible("editor-result-group-columns", cx),
        "分组页无分组键时仍应有分组列下拉"
    );

    // 7) 工具栏 SQL 入口：有 SQL 时第一格是固定文案「显示 SQL」（不再摊开 SQL 原文）
    cx.update(|_, app| {
        panel.update(app, |p, _| {
            p.result_sql = Some("select id, amount from t where amount > 100".to_string())
        })
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(
        visible("editor-result-sql-preview", cx),
        "有 SQL 时筛选框第 1 格应渲染「显示 SQL」入口"
    );
}

/// 【P2-3】多选集合推进：Ctrl 点击增删、普通点击单选替换、结果升序去重
#[test]
fn multi_selection_toggles_with_ctrl_and_replaces_without() {
    use crate::column_browser::next_multi_selection;

    // 单选替换：不管原先选了多少，普通点击只剩这一列
    assert_eq!(next_multi_selection(&[], 2, false), vec![2]);
    assert_eq!(next_multi_selection(&[0, 5], 2, false), vec![2]);

    // Ctrl 加选：追加并升序（面板与批量动作都按数据列序处理）
    assert_eq!(next_multi_selection(&[], 2, true), vec![2]);
    assert_eq!(next_multi_selection(&[2], 0, true), vec![0, 2]);
    assert_eq!(next_multi_selection(&[2, 7], 4, true), vec![2, 4, 7]);

    // Ctrl 再点已在集合里的列 = 取消；取消到空允许（用户可能清空后重新框选）
    assert_eq!(next_multi_selection(&[0, 2], 2, true), vec![0]);
    assert_eq!(next_multi_selection(&[2], 2, true), Vec::<usize>::new());

    // 重复项不会累积（同一列 Ctrl 加两次回到原集合）
    assert_eq!(next_multi_selection(&[0, 2], 2, true), vec![0]);
}

#[test]
fn hex_page_clamps_to_the_last_page() {
    use crate::panel::tabs::hex_page_clamp;

    // 空值：也算 1 页（页签显示「第 1/1 页」，不出现「第 1/0 页」）
    assert_eq!(hex_page_clamp(0, 0), (0, 1));
    assert_eq!(hex_page_clamp(0, 5), (0, 1), "空值下任何页号都落到 0");

    // 整页边界：恰好 256 字节 = 1 页；多 1 字节就开第 2 页
    assert_eq!(hex_page_clamp(256, 0), (0, 1));
    assert_eq!(hex_page_clamp(257, 0), (0, 2));
    assert_eq!(hex_page_clamp(257, 1), (1, 2));
    assert_eq!(hex_page_clamp(257, 9), (1, 2), "越界页号夹到末页");

    // 1KB = 4 页（每页 16 行 × 16 字节）
    assert_eq!(hex_page_clamp(1024, 3), (3, 4));
    assert_eq!(hex_page_clamp(1024, 4), (3, 4));
}

/// 【选中行 2026-11-18】接线用例：组件 `TableEvent::SelectRow` → 面板订阅 → delegate
/// 镜像 → `render_tr` 整行铺色。
///
/// 为什么必须有这一条（而不只是测 delegate 字段）：面板是在**组件自己的事件回调**里
/// 再 `grid.update` 一次，只有 gpui-fast 的 `Context::emit` 是**延迟派发**
/// （`context.rs:738-753`，只把 `Effect::Emit` 排进 `pending_effects`）时这才成立；
/// 万一将来改成同步派发，这条用例会以 double-lease panic 报错，而不是静默少一块底色。
/// 另：`#` 列点第二下升级成整行选中是选整行的唯一入口（`grid/status.rs` 关了组件窄行头），
/// 所以这条链路断了就等于「整行选中没有反馈」。
#[gpui_kit::test]
fn select_row_event_mirrors_into_the_delegate(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let (panel, cx) = panel_harness(cx);
    cx.update(|window, cx| window.draw(cx).clear(cx));
    // 网格句柄先取出来：`panel.read(app).grid().update(app, ..)` 会同时借 app 两次
    let grid = cx.update(|_window, app| panel.read(app).grid().clone());

    // 起点：没有整行选中（否则后面的断言可能被上一帧的残留糊弄过去）
    let before = cx.update(|_window, app| grid.read(app).delegate().selected_view_row());
    assert_eq!(before, None, "空起点：未选中整行时 delegate 不该有行号");

    // 走生产入口：组件 `set_selected_row` 自己会 emit `TableEvent::SelectRow`
    //（第 1 行是奇数行，正好能验证「选中压掉斑马纹」）
    cx.update(|_window, app| {
        grid.update(app, |state, cx| state.set_selected_row(1, cx));
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));

    let mirrored = cx.update(|_window, app| grid.read(app).delegate().selected_view_row());
    assert_eq!(
        mirrored,
        Some(1),
        "面板订阅没把 `SelectRow` 镜像进 delegate（整行选中会没有底色）"
    );
    assert!(
        cx.debug_bounds("editor-result-row-selected").is_some(),
        "选中行哨兵不在地：`render_tr` 没走选中分支"
    );
    // 「选中压掉斑马纹」的排他性交给 `grid/tests.rs::selected_row_paints_a_whole_row_band`
    // 断言（那边整段不重绘，首帧就是唯一口径）。这里不能断言 `row-odd` 缺席：
    // 本条先画过一帧未选中的画面，`debug_bounds` 的登记是**跨帧累积**的，
    // 上一帧的斑马行会一直留在册子里。
}

/// 【B15 接线】筛选框的 `InputEvent::Change` 订阅必须真的挂着。
///
/// 早先句柄写成 `let filter_sub = …; let _ = filter_sub;`：`Subscription` 是「掉了就
/// 解订」的守卫，`let _ =` 等于当场丢弃 ⇒ 真敲键一次也到不了 `on_filter_input`，
/// 「边打边筛」从未生效。编辑器侧那条 `filtering_waits_for_the_debounce` 直接调
/// `on_filter_input`，绕过了订阅，所以一直没暴露——本条走**真输入框 + 真敲键**。
#[gpui_kit::test]
fn typing_into_the_filter_box_reaches_the_panel(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let (panel, cx) = panel_harness(cx);
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let grid = cx.update(|_window, app| panel.read(app).grid().clone());

    // 焦点交给真筛选框（`TextareaState::focus` 内部走 focus_handle）
    cx.update(|window, cx| {
        let input = panel.read(cx).filter_input.clone();
        input.update(cx, |state, cx| state.focus(window, cx));
    });
    cx.simulate_keystrokes("1");
    cx.update(|window, cx| window.draw(cx).clear(cx));

    let typed = cx.update(|_window, cx| panel.read(cx).filter_text.clone());
    assert_eq!(
        typed, "1",
        "敲进筛选框的字没到面板：`InputEvent::Change` 订阅没挂着（边打边筛失效）"
    );

    // 防抖到点后才应用：2 行里只有 `1` 那行匹配
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(400));
    cx.run_until_parked();
    let rows = cx.update(|_window, app| grid.read(app).delegate().view_total());
    assert_eq!(rows, 1, "防抖到点后筛选应生效（`2 / NULL` 那行要被筛掉）");
}

/// 【筛选历史 2026-11-18】Enter 提交：立即应用 + 记进历史。
///
/// 对齐 DBeaver：Enter 触发 `setCustomDataFilter()`（`ResultSetFilterPanel.java:296`）；
/// 历史条在 `:214` 的 `HistoryPanel`（点开是历史表 `createFilterHistoryPanel`）。
/// 本仓这条的口径：
/// 1. Enter **不等防抖**（立即生效）；
/// 2. 只有显式提交入历史 —— 打字过程中的防抖状态不入（否则历史被 `a`/`ab`/`abc` 灌满）；
/// 3. 同一个词重复提交不重复记（提到最前）。
#[gpui_kit::test]
fn commit_on_enter_records_filter_history(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let (panel, cx) = panel_harness(cx);
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let grid = cx.update(|_window, app| panel.read(app).grid().clone());

    // 真敲键进筛选框（走订阅，不绕过 `on_filter_input`）
    cx.update(|window, cx| {
        let input = panel.read(cx).filter_input.clone();
        input.update(cx, |state, cx| state.focus(window, cx));
    });
    cx.simulate_keystrokes("1");
    cx.update(|window, cx| window.draw(cx).clear(cx));

    // 此时还没到防抖点：网格未筛、历史为空（打字不算提交）
    let before = cx.update(|_window, app| {
        (
            grid.read(app).delegate().view_total(),
            panel.read(app).filter_history().to_vec(),
        )
    });
    assert_eq!(before.0, 2, "防抖未到点时不该已经筛掉行");
    assert!(before.1.is_empty(), "打字过程中的状态不该进历史");

    // Enter = 提交：立即应用 + 入历史
    cx.simulate_keystrokes("enter");
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let after = cx.update(|_window, app| {
        (
            grid.read(app).delegate().view_total(),
            panel.read(app).filter_history().to_vec(),
        )
    });
    assert_eq!(after.0, 1, "Enter 应立刻应用筛选（不等 300ms 防抖）");
    assert_eq!(
        after.1,
        vec!["1".to_string()],
        "Enter 提交的筛选词应记进历史（新的在前）"
    );
}

/// 【筛选历史 2026-11-18】历史口径：去重置顶 + 封顶 [`FILTER_HISTORY_LIMIT`]，
/// 以及「从历史里选一条」会回填输入框并立即应用。
#[gpui_kit::test]
fn filter_history_dedupes_caps_and_replays(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let (panel, cx) = panel_harness(cx);
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let grid = cx.update(|_window, app| panel.read(app).grid().clone());

    // 灌 10 个词：历史只留最后 8 条（新的在前），且 `k1` 重复提交会被提回最前
    cx.update(|_window, cx| {
        panel.update(cx, |panel, _cx| {
            for k in 0..10 {
                panel.filter_text = format!("k{k}");
                panel.note_filter_commit();
            }
            // 重复提交最老的那条：应提到最前而不是新增一条
            panel.filter_text = "k2".to_string();
            panel.note_filter_commit();
        });
    });
    let history = cx.update(|_window, app| panel.read(app).filter_history().to_vec());
    assert_eq!(
        history.len(),
        crate::panel::data::FILTER_HISTORY_LIMIT,
        "历史应封顶在 FILTER_HISTORY_LIMIT 条"
    );
    assert_eq!(history[0], "k2", "重复提交的词应提到最前，而不是追加一条");
    assert!(
        !history.contains(&"k1".to_string()),
        "被提上来那条之外的名次保持原序（k1 已在封顶时被挤掉）"
    );

    // 从历史里选一条：回填输入框 + 立即应用
    cx.update(|window, cx| {
        panel.update(cx, |panel, cx| {
            panel.apply_filter_from_history("k2", window, cx);
        });
    });
    let replayed = cx.update(|_window, app| {
        (
            panel.read(app).filter_text.clone(),
            panel
                .read(app)
                .filter_input
                .read(app)
                .value()
                .to_string(),
            grid.read(app).delegate().view_total(),
        )
    });
    assert_eq!(replayed.0, "k2", "面板筛选词应切到选中的历史");
    assert_eq!(replayed.1, "k2", "输入框应回填（所见即所得）");
    assert_eq!(
        replayed.2, 0,
        "`k2` 不在两行数据里，应用后应筛成 0 行（证明真的下去了）"
    );
}

/// 【结果内查找 2026-11-18】查找栏走通：打开 → 真敲键 → 命中数 + 选中第一个命中
/// → 下一个 → 上一个。数据是 `["1","1234.5"] / ["2","NULL"]`，词 `"1"` 命中两格
/// （`id=1` 与 `amount=1234.5`），正好够验「步进」与「回卷」。
#[gpui_kit::test]
fn find_bar_locates_matches_and_steps_through_them(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let (panel, cx) = panel_harness(cx);
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let grid = cx.update(|_window, app| panel.read(app).grid().clone());

    // 1) 打开查找栏（焦点交给查找框）
    cx.update(|window, cx| panel.update(cx, |p, cx| p.toggle_find(window, cx)));
    assert!(
        cx.update(|_window, app| panel.read(app).find_state().0),
        "打开后 `find_open` 应为真"
    );

    // 2) 真敲键：词进面板、命中数算出来、立刻选中第一个命中（`#` 槽旁边的 id 列）
    cx.update(|window, cx| {
        let input = panel.read(cx).find_input.clone();
        input.update(cx, |state, cx| state.focus(window, cx));
    });
    cx.simulate_keystrokes("1");
    cx.update(|window, cx| window.draw(cx).clear(cx));

    let after_type = cx.update(|_window, app| {
        let (open, query, hits) = panel.read(app).find_state();
        (
            open,
            query.to_string(),
            hits,
            panel.read(app).find_at,
            grid.read(app).selected_cell(),
        )
    });
    assert!(after_type.0, "查找栏应仍开着");
    assert_eq!(after_type.1, "1", "敲进去的词应到面板（订阅真的挂着）");
    assert_eq!(
        after_type.2, 2,
        "`1` 命中两格：`id=1` 与 `amount=1234.5`（子串口径，大小写不敏感）"
    );
    assert_eq!(
        (after_type.3, after_type.4),
        (Some((0, 1)), Some((0, 1))),
        "换词后应定位到第一处命中，且网格真的被选中（不是只改面板状态）"
    );

    // 3) 下一处：从当前命中往后走 → 第二处（同行 amount 列）；再按一次回卷到第一处
    cx.update(|_window, cx| panel.update(cx, |p, cx| p.find_step(true, cx)));
    let next = cx.update(|_window, app| (panel.read(app).find_at, grid.read(app).selected_cell()));
    assert_eq!(next, (Some((0, 2)), Some((0, 2))), "下一处应是 amount 那格");
    cx.update(|_window, cx| panel.update(cx, |p, cx| p.find_step(true, cx)));
    let wrapped = cx.update(|_window, app| panel.read(app).find_at);
    assert_eq!(wrapped, Some((0, 1)), "找完一圈回卷到第一处");

    // 4) 上一处：反向绕过一圈回到末尾那格
    cx.update(|_window, cx| panel.update(cx, |p, cx| p.find_step(false, cx)));
    let prev = cx.update(|_window, app| panel.read(app).find_at);
    assert_eq!(prev, Some((0, 2)), "上一处从第一处回卷到最后一处");

    // 5) 没有命中的词：计数归零、导航不动作（仍是上一次的命中位置）
    cx.update(|_window, cx| panel.update(cx, |p, cx| p.find_query_changed("zzz".into(), cx)));
    let none = cx.update(|_window, app| panel.read(app).find_state().2);
    assert_eq!(none, 0, "`zzz` 一个也不命中");

    // 6) 关掉查找栏：状态回落，词保留（DBeaver 浮层 close 也不清词）
    cx.update(|_window, cx| panel.update(cx, |p, cx| p.close_find(cx)));
    let closed = cx.update(|_window, app| {
        let (open, query, _hits) = panel.read(app).find_state();
        (open, query.to_string())
    });
    assert!(!closed.0, "关掉后 `find_open` 应为假");
    assert_eq!(closed.1, "zzz", "关闭时保留查找词，下次打开接着用");
}

/// 【2026-11-18 侧栏结构对齐】左缘竖条两段式：展示组（网格 / 文本）在上，
/// **记录模式沉底**。对齐 DBeaver `VerticalFolder(SWT.LEFT)`：
/// - 逐条摆 `VerticalButton(SWT.LEFT | SWT.RADIO)` 展示按钮（`ResultSetViewer.java:948-962`）
/// - `createEmptyLabel(..., FILL_VERTICAL)` 弹簧（`:963`）
/// - 记录模式按钮在弹簧之后（`:964-965`）
///
/// 这里量**真实像素**：只钉相对关系（网格在文本上面、记录远在文本之下），
/// 不钉具体 y（竖条总高由面板高度决定，窗口尺寸一变就漂）。
#[gpui_kit::test]
fn left_bar_puts_presentation_group_on_top_and_record_mode_at_the_bottom(
    cx: &mut gpui_kit::TestAppContext,
) {
    use gpui_kit::test::TestWindowExt as _;
    use gpui_kit::{ElementId, SharedString};

    cx.update(gpui_kit::init);
    // 这一条只看竖条内元素的相对高低，不用面板实体
    let (_panel, cx) = panel_harness(cx);
    cx.update(|window, cx| window.draw(cx).clear(cx));

    let top_of = |id: &str, cx: &mut gpui_kit::VisualTestContext| -> Option<f32> {
        cx.update(|window, _| {
            window
                .try_find(ElementId::Name(SharedString::from(id.to_string()))
                    as ElementId)
                .map(|snap| f32::from(snap.bounds().top()))
        })
    };
    let grid = top_of("editor-result-bar-grid", cx).expect("竖条网格按钮应已渲染");
    let text = top_of("editor-result-bar-text", cx).expect("竖条文本按钮应已渲染");
    let record = top_of("editor-result-bar-record", cx).expect("竖条记录按钮应已渲染");

    assert!(
        grid < text,
        "展示组顺序：网格在上、文本在下（{grid} vs {text}）"
    );
    // 弹簧把记录模式推到底部：它与文本按钮之间至少隔开一整个按钮高（20px 档），
    // 没弹簧时三个按钮是紧邻的（差 20~24px），有弹簧时差几百 px
    assert!(
        record - text > 60.0,
        "记录模式应在弹簧之后沉底（文本 {text} → 记录 {record}，差 {}px）",
        record - text
    );
    // 记录导航默认不摆（只有转置激活时才出现）
    assert!(
        top_of("editor-result-bar-record-prev", cx).is_none(),
        "未进记录视图时不该摆行导航"
    );
}

/// 【2026-11-18 工具栏结构对齐】筛选清除钮 = **常驻 + 空词禁用**：
/// DBeaver 的 `filtersClearButton` 建出来就是禁用态（`ResultSetFilterPanel.java:319-327`），
/// 有筛选后才由 `enablePanelControls` 打开（`:449-454`）。此前本仓是「有词才出现」，
/// 位置随打字跳一格。此用例钉住「一直在、状态跟着词走」。
#[gpui_kit::test]
fn filter_clear_button_is_always_there_and_gated_by_the_filter_text(
    cx: &mut gpui_kit::TestAppContext,
) {
    use gpui_kit::test::TestWindowExt as _;
    use gpui_kit::{ElementId, SharedString};

    cx.update(gpui_kit::init);
    let (panel, cx) = panel_harness(cx);
    cx.update(|window, cx| window.draw(cx).clear(cx));

    let clear_state = |cx: &mut gpui_kit::VisualTestContext| -> Option<Option<bool>> {
        cx.update(|window, _| {
            window
                .try_find(ElementId::Name(SharedString::from(
                    "editor-result-filter-clear".to_string(),
                )))
                .map(|snap| snap.disabled())
        })
    };

    // 空词：按钮在，且处于禁用态
    let empty = clear_state(cx).expect("清除钮应常驻（不再按词出现）");
    assert_eq!(empty, Some(true), "空词时清除钮应禁用");

    // 有词：同一个按钮转成可用
    cx.update(|_, app| {
        panel.update(app, |p, cx| {
            p.filter_text = "1".to_string();
            // 手动改状态后要自己 notify：否则 `draw` 复用旧帧，读到旧树
            cx.notify();
        })
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let filled = clear_state(cx).expect("有词时清除钮仍应在原位");
    // 实测口径：可用态**不暴露** a11y disabled 位（`None`），只有禁用时才写 `Some(true)`；
    // 所以这里钉「不再是禁用」，而不是钉 `Some(false)`
    assert_ne!(
        filled,
        Some(true),
        "有词时清除钮不该再是禁用态（可用态不暴露 disabled 位）"
    );
}

/// 【2026-11-18 工具栏结构对齐】筛选条 ✕ 的箭头菜单条目（纯函数）：
/// DBeaver 的 `EraseItemListener` 菜单是清筛选设置 / 显示隐藏列 / 重置颜色 /
/// transformers / pins / 全部重置（`ResultSetViewer.java:1525-1545`）。
/// 本仓「能力没有就不摆」只留真有落点的两条，且都按条件出现：
/// 有筛选词才有「清除筛选词」，有隐藏列才有「显示全部列」。
#[gpui_kit::test]
fn erase_menu_lists_only_the_entries_this_repo_can_honour(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);

    // 空词 + 无隐藏列：菜单里什么都没有（渲染侧摆一条禁用说明）
    assert!(
        super::events::erase_menu_labels("", 0).is_empty(),
        "没有可清除项时清单应为空"
    );
    assert!(
        super::events::erase_menu_labels("   ", 0).is_empty(),
        "空白筛选词不算有词"
    );

    // 有词：清除筛选词
    assert_eq!(
        super::events::erase_menu_labels("abc", 0),
        vec!["清除筛选词"],
        "有筛选词时应有清除项"
    );

    // 有隐藏列：显示全部列（与列头菜单同口径，不看筛选词）
    assert_eq!(
        super::events::erase_menu_labels("", 2),
        vec!["显示全部列"],
        "有隐藏列时应能一键放回"
    );

    // 两者都有：两条都在，顺序固定（清除在前）
    assert_eq!(
        super::events::erase_menu_labels("abc", 1),
        vec!["清除筛选词", "显示全部列"],
        "两条都有时顺序固定"
    );
}

/// 【2026-11-18 侧栏结构对齐】右缘竖条顶部的**面板总开关**：
/// DBeaver 竖条第一颗就是总开关 `panelsButton`（CHECK，checked = 面板可见，
/// 点击 `showPanels(!isPanelsVisible())`，`ResultSetViewer.java:1038-1056`）。本仓此前
/// 只有状态栏右端的三态开关，竖条顶部缺这颗。这里钉住「按钮在竖条里 + 收 / 放两向都到位」。
#[gpui_kit::test]
fn right_edge_bar_carries_the_master_panels_toggle(cx: &mut gpui_kit::TestAppContext) {
    use gpui_kit::test::TestWindowExt as _;
    use gpui_kit::{ElementId, SharedString};

    cx.update(gpui_kit::init);
    let (panel, cx) = panel_harness(cx);
    // 右缘竖条的挂载条件是 `edge_bar_visible && result_can_copy`（有网格结果才挂），
    // 测试壳默认 can_copy = false，这里先打开再出帧
    cx.update(|_, app| {
        panel.update(app, |p, cx| {
            p.result_can_copy = true;
            cx.notify();
        })
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));

    // 默认（测试壳）面板可见 → 总开关在竖条里，且竖条本身也挂着
    let master = cx.update(|window, _| {
        window.try_find(ElementId::Name(SharedString::from(
            "editor-result-edge-panels".to_string(),
        )))
    });
    assert!(master.is_some(), "右缘竖条应摆面板总开关");
    assert!(
        cx.update(|_, app| panel.read(app).panels_visible()),
        "默认面板应可见"
    );

    // 关：整条收起（与状态栏三态开关的收起态同一定义）
    cx.update(|_, app| panel.update(app, |p, cx| p.set_panels_visible(false, cx)));
    assert!(
        !cx.update(|_, app| panel.read(app).panels_visible()),
        "关掉总开关后应视为不可见"
    );

    // 开回来：竖条 + 内容一起回来
    cx.update(|_, app| panel.update(app, |p, cx| p.set_panels_visible(true, cx)));
    assert!(
        cx.update(|_, app| panel.read(app).panels_visible()),
        "再打开后应恢复可见"
    );
    // 幂等：已经可见时再「打开」不该改任何状态
    cx.update(|_, app| panel.update(app, |p, cx| p.set_panels_visible(true, cx)));
    assert!(cx.update(|_, app| panel.read(app).panels_visible()));
}

/// 【2026-11-18 工具栏结构对齐】筛选条「状态历史」的**状态机**（纯逻辑，不起窗口）：
/// 对齐 DBeaver `setNewState`（`ResultSetViewer.java:2419-2442`）+ `navigateHistory`
/// （`:4050-4063`）+ 启用态（`:456-475`）：
/// - 同一个 SQL 重复进栈 → 只挪位置，不追加
/// - 从中间走新路 → 前向历史作废（浏览器式）
/// - 空 SQL / 无 SQL → 不入历史
#[gpui_kit::test]
fn state_history_dedupes_steps_and_drops_the_forward_branch(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let (panel, cx) = panel_harness(cx);

    let note = |cx: &mut gpui_kit::VisualTestContext, sql: Option<&str>| {
        let sql = sql.map(|s| s.to_string());
        cx.update(|_, app| {
            panel.update(app, |p, _| {
                p.result_sql = sql.clone();
                p.note_state_sql();
            })
        });
    };

    // 空 SQL（写语句 / 无结论）不该产生历史项
    note(cx, None);
    note(cx, Some("   "));
    assert_eq!(cx.update(|_, app| panel.read(app).history_len()), 0);

    // 三个状态依次进来：A → B → C，位置在 C（能后退、不能前进）
    note(cx, Some("select 1"));
    note(cx, Some("select 2"));
    note(cx, Some("select 3"));
    let (len, pos, can_back, can_forward) = cx.update(|_, app| {
        let p = panel.read(app);
        (p.history_len(), p.history_pos(), p.history_can_back(), p.history_can_forward())
    });
    assert_eq!((len, pos), (3, 2));
    assert!(can_back && !can_forward, "在末位：该能后退不能前进");

    // 后退一格 → 目标就是 B（宿主拿它重跑 SQL）
    let back = cx.update(|_, app| panel.read(app).history_target(-1));
    assert_eq!(
        back,
        Some((1, "select 2".to_string())),
        "后退一格应指向中间那条状态"
    );
    // 前进一格 → 从末位出发没有前向
    assert_eq!(cx.update(|_, app| panel.read(app).history_target(1)), None);

    // 面板把位置交给宿主（宿主执行完回调），这里直接模拟落位到 B
    cx.update(|_, app| panel.update(app, |p, cx| p.set_history_pos(1, cx)));
    let back = cx.update(|_, app| panel.read(app).history_target(-1));
    let forward = cx.update(|_, app| panel.read(app).history_target(1));
    assert_eq!(back, Some((0, "select 1".to_string())));
    assert_eq!(forward, Some((2, "select 3".to_string())));

    // 从中间（B）走一条新路 D：C 的前向历史作废，历史变 A / B / D
    note(cx, Some("select 4"));
    let (len, pos) = cx.update(|_, app| (panel.read(app).history_len(), panel.read(app).history_pos()));
    assert_eq!((len, pos), (3, 2), "新状态应截断前向再追加");
    assert_eq!(
        cx.update(|_, app| panel.read(app).history_target(-1)),
        Some((1, "select 2".to_string())),
        "新路的上一格仍是 B"
    );
    assert_eq!(
        cx.update(|_, app| panel.read(app).history_target(1)),
        None,
        "C 已被截断，不能前进"
    );
    // 回到 A（去重：不追加、只挪位置）
    note(cx, Some("select 1"));
    let (len, pos) = cx.update(|_, app| (panel.read(app).history_len(), panel.read(app).history_pos()));
    assert_eq!((len, pos), (3, 0), "重复状态只挪位置，不追加");
}

/// 【2026-11-18 工具栏结构对齐】后退 / 前进两个**分裂按钮**在筛选行里常驻、启用态跟位置走：
/// DBeaver 的那一对是 `SWT.DROP_DOWN`（主体走一格 + caret 弹「该方向全部可达状态」），
/// 只有 `pos > 0` / `pos < size - 1` 才打开（`ResultSetViewer.java:456-475`）。
/// 这里钉住：主体按钮在位与禁用态、方向菜单条目（近的在前）、tooltip / 标签口径
/// （`describeState`，`:5457-5473`：折叠空白 + 50 字符截断）。
#[gpui_kit::test]
fn history_buttons_sit_in_the_filter_row_and_gate_on_position(cx: &mut gpui_kit::TestAppContext) {
    use gpui_kit::test::TestWindowExt as _;
    use gpui_kit::{ElementId, SharedString};

    cx.update(gpui_kit::init);
    let (panel, cx) = panel_harness(cx);
    cx.update(|window, cx| window.draw(cx).clear(cx));

    // 主体（分裂按钮左半）的 id 才是带 a11y 禁用位的那一半
    let state = |cx: &mut gpui_kit::VisualTestContext, id: &str| -> Option<Option<bool>> {
        let name = SharedString::from(id.to_string());
        cx.update(|window, _| {
            window
                .try_find(ElementId::Name(name))
                .map(|snap| snap.disabled())
        })
    };

    // 空历史：两个主体都在，都禁用
    assert_eq!(
        state(cx, "editor-result-history-back-step"),
        Some(Some(true)),
        "空历史时后退钮应在位且禁用"
    );
    assert_eq!(
        state(cx, "editor-result-history-forward-step"),
        Some(Some(true)),
        "空历史时前进钮应在位且禁用"
    );
    // 空历史：两个方向的菜单都空
    assert!(
        cx.update(|_, app| panel.read(app).history_menu_items(-1))
            .is_empty(),
        "空历史时后退菜单为空"
    );
    assert!(
        cx.update(|_, app| panel.read(app).history_menu_items(1))
            .is_empty(),
        "空历史时前进菜单为空"
    );

    // 灌两条状态 → 位置在末位：后退可用、前进仍禁用
    cx.update(|_, app| {
        panel.update(app, |p, cx| {
            p.result_sql = Some("select 1".to_string());
            p.note_state_sql();
            p.result_sql = Some("select 2".to_string());
            p.note_state_sql();
            cx.notify();
        })
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert_ne!(
        state(cx, "editor-result-history-back-step"),
        Some(Some(true)),
        "有历史可退时后退钮不该再禁用"
    );
    assert_eq!(
        state(cx, "editor-result-history-forward-step"),
        Some(Some(true)),
        "在末位时前进钮仍禁用"
    );

    // tooltip 口径：目标状态描述（折叠空白、超 50 字符截断）
    let hint = cx.update(|_, app| panel.read(app).history_hint(-1));
    assert_eq!(hint, "select 1", "tooltip 应是目标状态 SQL");
    assert_eq!(
        super::events::state_label("select\n  1\n   from t"),
        "select 1 from t",
        "空白应折叠成单空格"
    );
    let long = "x".repeat(80);
    let label = super::events::state_label(&long);
    assert_eq!(label.chars().count(), 51, "50 字符 + 省略号");
    assert!(label.ends_with('…'));

    // 方向菜单：在末位时后退方向 = 全部前序状态，且近的在前；前进方向为空
    let back_menu = cx.update(|_, app| panel.read(app).history_menu_items(-1));
    assert_eq!(
        back_menu,
        vec![(0usize, "select 1".to_string())],
        "后退菜单应列出该方向全部可达状态（近的在前）"
    );
    assert!(
        cx.update(|_, app| panel.read(app).history_menu_items(1))
            .is_empty(),
        "末位没有前向状态，前进菜单应为空"
    );
}

/// 【2026-11-18 侧栏结构对齐】右栏每页工具条右端的**视图菜单**（▾）：
/// DBeaver 在 `addPanel` 里给**每个面板**的工具条右端都挂一颗 `IMG_LCL_VIEW_MENU`
/// 按钮（`addDefaultPanelActions`，`ResultSetViewer.java:1590-1614`），点开是
/// `fillPanelsMenu()`（`:1559-1588`）生成的菜单：全部面板的 CHECK 项 → 分隔 →
/// `CMD_TOGGLE_MAXIMIZE` → `CMD_ACTIVATE_PANELS` → 分隔 → `CMD_TOGGLE_PANELS` →
/// `CMD_TOGGLE_LAYOUT`。本仓最大化 / 激活面板两项还没有落地端口（按「能力没有就不摆」
/// 不列），其余每一项都有真请求落点。
///
/// 这里钉三件事：① 菜单条目顺序（纯函数）；② 条目文案随状态报「点了会做什么」；
/// ③ 六页的页内工具条都真的挂上了这颗按钮（`PopupMenu` 内部不可读，形态断言走纯函数）。
#[gpui_kit::test]
fn right_panel_view_menu_lists_pages_then_the_two_toggles(cx: &mut gpui_kit::TestAppContext) {
    use gpui_kit::test::TestWindowExt as _;
    use gpui_kit::{ElementId, SharedString};

    cx.update(gpui_kit::init);

    // ① 条目顺序：六页 → 分隔 → 面板总开关 → 布局方向
    let items = super::events::panel_view_menu_items();
    let pages: Vec<RightTab> = items
        .iter()
        .filter_map(|item| match item {
            super::events::PanelViewItem::Page(tab) => Some(*tab),
            _ => None,
        })
        .collect();
    assert_eq!(
        pages,
        vec![
            RightTab::Browser,
            RightTab::Value,
            RightTab::Stats,
            RightTab::Group,
            RightTab::Reference,
            RightTab::ExecSql,
        ],
        "面板勾选项的顺序应与竖条 / 页签同一份（列浏览器在前）"
    );
    assert_eq!(items.len(), 9, "六页 + 分隔 + 面板开关 + 布局方向");
    assert_eq!(items[6], super::events::PanelViewItem::Separator);
    assert_eq!(items[7], super::events::PanelViewItem::PanelsToggle);
    assert_eq!(items[8], super::events::PanelViewItem::LayoutToggle);
    // 没有最大化 / 激活面板：本仓没有端口，不摆点了没反应的条目
    assert!(
        items
            .iter()
            .all(|item| matches!(item, super::events::PanelViewItem::Page(_))
                || matches!(
                    item,
                    super::events::PanelViewItem::Separator
                        | super::events::PanelViewItem::PanelsToggle
                        | super::events::PanelViewItem::LayoutToggle
                )),
        "菜单里只应有四种条目"
    );

    // ② 文案：页用页面名，两个开关报「点了会做什么」
    let label = super::events::panel_view_item_label;
    let page = super::events::PanelViewItem::Page(RightTab::Stats);
    assert_eq!(label(page, true, false), "计算");
    let panels = super::events::PanelViewItem::PanelsToggle;
    assert_eq!(
        label(panels, true, false),
        "收起结果面板",
        "面板可见时报「收起」"
    );
    assert_eq!(
        label(panels, false, false),
        "展开结果面板",
        "面板收起时报「展开」"
    );
    let layout = super::events::PanelViewItem::LayoutToggle;
    assert_eq!(label(layout, true, false), "切换为竖排布局（上下分栏）");
    assert_eq!(label(layout, true, true), "切换为横排布局（左右分栏）");
    assert_eq!(label(super::events::PanelViewItem::Separator, true, false), "");

    // ③ 六页的页内工具条都挂上了视图菜单（id = `<页工具条 id>-view-menu`）
    let (panel, cx) = panel_harness(cx);
    for (tab, toolbar_id) in [
        (RightTab::Browser, "editor-result-browser-toolbar"),
        (RightTab::Value, "editor-result-value-toolbar"),
        (RightTab::Stats, "editor-result-stats-toolbar"),
        (RightTab::Group, "editor-result-group-toolbar"),
        (RightTab::Reference, "editor-result-reference-toolbar"),
        (RightTab::ExecSql, "editor-result-exec-sql-toolbar"),
    ] {
        cx.update(|_, app| panel.update(app, |p, cx| p.set_right_tab(tab, cx)));
        cx.update(|window, cx| window.draw(cx).clear(cx));
        let menu_id = SharedString::from(format!("{toolbar_id}-view-menu"));
        let found = cx.update(|window, _| window.try_find(ElementId::Name(menu_id)));
        assert!(
            found.is_some(),
            "{toolbar_id} 右端应有视图菜单按钮（{toolbar_id}-view-menu）"
        );
    }
}

/// 【2026-11-18 侧栏结构对齐】右栏页签条**列出全部六页**且点页签即切页：
/// DBeaver 的右栏是一个 `CTabFolder panelFolder`（`ResultSetViewer.java:1430-1478`），
/// 六页都作为页签常驻其中（`addPanel` 里 `new CTabItem(panelFolder, SWT.CLOSE)`），
/// 点页签走 `setActivePanel`。此前本仓横条只显示当前页、`on_click` 是空实现（点了没反应）。
///
/// 断言走**真实像素点击**：把当前页切到最后一个（执行SQL），再点页签行最左侧，
/// 必须切回第一个（列浏览器）——既证明六个页签真的排在这一行里，也证明点击真接了线。
#[gpui_kit::test]
fn right_panel_tab_strip_lists_all_panels_and_click_switches(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let (panel, cx) = panel_harness(cx);

    // 当前页 = 执行SQL（最后一个页签）
    cx.update(|_, app| panel.update(app, |p, cx| p.set_right_tab(RightTab::ExecSql, cx)));
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert_eq!(
        cx.update(|_, app| panel.read(app).right_tab),
        RightTab::ExecSql,
        "先把当前页切到执行SQL"
    );

    let bar = cx
        .debug_bounds("editor-result-right-tabs")
        .expect("页签行应带调试选择器才量得到");
    // 页签行最左一格一定是第一个页签（列浏览器）：点它应把「切到列浏览器」入队
    // （切页是宿主动作：面板只 `queue.request`，由宿主 dispatch 调 `set_right_tab`，
    // 所以这里断言队列，而不是面板状态）
    let point = gpui_kit::point(bar.origin.x + px(24.), bar.origin.y + bar.size.height / 2.);
    cx.simulate_click(point, gpui_kit::Modifiers::default());
    cx.update(|window, cx| window.draw(cx).clear(cx));

    let queued = cx.update(|_, app| panel.read(app).queue.drain());
    assert!(
        queued
            .iter()
            .any(|request| matches!(request, crate::ports::ResultRequest::SetRightTab(RightTab::Browser))),
        "点第一个页签应入队「切到列浏览器」（点空白 / 死回调都会让队列为空）：{queued:?}，页签行 {bar:?}"
    );
}

/// 【状态栏翻页对齐】DBeaver 底部动作条 `toolbar:…resultset.status.navCmds`
/// （`plugin.xml:1470-1512`）的行导航是四颗：`row.first` → `row.previous` → `row.next` →
/// `row.last`，其后才是 `fetch.page` / `fetch.all`。本仓此前只有三颗（缺 `row.next`，
/// 且「末页」被摆到取数钮之后），本批补齐并归位。
///
/// 断言用**真实像素横坐标**（`ElementSnapshot::bounds()`）：顺序是布局事实，
/// 只看「在不在」抓不到被摆错位；而且「末页在取下一段 / 取全部之前」这条
/// 只有量 x 才守得住。
#[gpui_kit::test]
fn status_row_pager_follows_dbeaver_nav_order(cx: &mut gpui_kit::TestAppContext) {
    use gpui_kit::test::TestWindowExt as _;
    use gpui_kit::{ElementId, SharedString};

    cx.update(gpui_kit::init);
    let (panel, cx) = panel_harness(cx);
    // 有行才注入四颗分页钮（`has_rows = status.total_rows > 0`）
    cx.update(|_, app| {
        panel.update(app, |p, cx| {
            p.status = Some(crate::grid::status::ResultStatus {
                total_rows: 2,
                ..Default::default()
            });
            cx.notify();
        })
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));

    let left_of = |cx: &mut gpui_kit::VisualTestContext, id: &str| -> f32 {
        let name = SharedString::from(id.to_string());
        let snap = cx
            .update(|window, _| window.try_find(ElementId::Name(name)))
            .unwrap_or_else(|| panic!("状态栏应有 {id}"));
        f32::from(snap.bounds().origin.x)
    };

    let first = left_of(cx, "editor-result-page-first");
    let prev = left_of(cx, "editor-result-page-prev");
    let next = left_of(cx, "editor-result-page-next");
    let last = left_of(cx, "editor-result-page-last");
    assert!(
        first < prev && prev < next && next < last,
        "分页四钮应按 首页→上页→下页→末页 从左到右排（实际 {first}/{prev}/{next}/{last}）"
    );
}

/// 【筛选设置 · 列约束】页签切换 + 列清单投影跟着 delegate 的真实状态走。
///
/// 对齐 DBeaver `FilterSettingsDialog` 的 Columns 页签（`addColumn` 名称 `:129`、
/// `addBooleanColumn` 可见 `:148` / 固定 `:178`）。这里钉的是本仓这一侧的两件事实：
/// ① 页签状态真的切（默认停在列页，与 DBeaver `setSelection(0)` 一致）；
/// ② 列清单不是假的——隐藏 / 冻结改在网格 delegate 上，投影立刻反映出来。
#[gpui_kit::test]
fn filter_settings_columns_tab_projects_the_grid_column_state(cx: &mut gpui_kit::TestAppContext) {
    use crate::ports::FilterSettingsTab;

    cx.update(gpui_kit::init);
    let (panel, cx) = panel_harness(cx);

    // 默认页签 = 列约束（DBeaver 对话框先建列页）
    assert_eq!(
        panel.read_with(cx, |p, _| p.filter_settings_tab),
        FilterSettingsTab::Columns,
        "浮层默认应停在列约束页"
    );

    // 切到自定义条件页再切回来（纯 UI 状态，不入任何请求）
    cx.update(|_window, app| {
        panel.update(app, |p, cx| {
            p.set_filter_settings_tab(FilterSettingsTab::Custom, cx)
        })
    });
    assert_eq!(
        panel.read_with(cx, |p, _| p.filter_settings_tab),
        FilterSettingsTab::Custom
    );
    cx.update(|_window, app| {
        panel.update(app, |p, cx| {
            p.set_filter_settings_tab(FilterSettingsTab::Columns, cx)
        })
    });
    assert!(
        panel.read_with(cx, |p, _| p.queue.is_empty()),
        "切页签不该入队请求"
    );

    // 列清单初始：两列都可见、都没冻结
    let rows = panel.read_with(cx, |p, app| p.column_constraint_rows(app));
    assert_eq!(
        rows,
        vec![
            ("id".to_string(), true, false),
            ("amount".to_string(), true, false)
        ],
        "初始列清单应是「两列都可见、未冻结」"
    );

    // 隐藏第 1 列（走与列浏览器同一通道的批量接口），投影应立刻变 false
    cx.update(|_window, app| {
        panel.update(app, |p, cx| p.set_columns_hidden(vec![0], true, cx))
    });
    let rows = panel.read_with(cx, |p, app| p.column_constraint_rows(app));
    assert!(!rows[0].1, "隐藏后 id 列应显示为不可见：{rows:?}");
    assert!(rows[1].1, "另一列不该被牵连：{rows:?}");

    // 冻结第 2 列（直接改 delegate，与列头菜单 ToggleFreeze 同一条路）
    cx.update(|_window, app| {
        let grid = panel.read(app).grid.clone();
        grid.update(app, |state, app| {
            state.delegate_mut().toggle_freeze(1);
            state.refresh(app);
        });
    });
    let rows = panel.read_with(cx, |p, app| p.column_constraint_rows(app));
    assert!(rows[1].2, "冻结后 amount 列应显示为已冻结：{rows:?}");
    assert!(!rows[0].2, "id 列不该被冻结：{rows:?}");
}

/// 【筛选设置】浮层开关 + 应用入队（对齐 DBeaver `filtersCustomButton` →
/// `FilterSettingsDialog` 的 Custom 页签：`whereText` / `orderText` → `setWhere` / `setOrder`）。
///
/// 面板只负责「读草稿 + 入队」，真正重查在宿主（`run_custom_filter` → 改写函数）；
/// 这里钉的是**面板这一侧的两件事实**：① 开关状态真的切换、浮层真的渲染；
/// ② 应用的请求带上「筛选框当前文本 + 排序草稿」两段，而不是只带其一。
#[gpui_kit::test]
fn filter_settings_open_close_and_apply_carry_the_draft(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let (panel, cx) = panel_harness(cx);

    // 1) 打开：标志位变真，浮层卡片真的进元素树
    cx.update(|window, app| {
        panel.update(app, |p, cx| p.open_filter_settings(window, cx));
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(
        cx.debug_bounds("editor-result-filter-settings-card").is_some(),
        "打开后应渲染筛选设置浮层"
    );
    assert!(
        panel.read_with(cx, |p, _| p.queue.is_empty()),
        "打开浮层不该入任何请求（它只是 UI 状态）"
    );

    // 2) 关闭：标志位回落，浮层从元素树消失
    cx.update(|_window, app| panel.update(app, |p, cx| p.close_filter_settings(cx)));
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(
        cx.debug_bounds("editor-result-filter-settings-card").is_none(),
        "关闭后浮层不该还挂着"
    );

    // 3) 应用：筛选文本进 where_text，排序草稿进 order_text，然后浮层自动关闭
    cx.update(|_window, app| {
        panel.update(app, |p, cx| p.on_filter_input("amount > 1".to_string(), cx))
    });
    cx.update(|window, app| {
        let input = panel.read(app).filter_order_input.clone();
        input.update(app, |state, cx| state.set_value("id DESC", window, cx));
    });
    cx.update(|_window, app| panel.update(app, |p, cx| p.apply_filter_settings(cx)));

    let drained = cx.update(|_window, app| panel.read(app).queue.drain());
    assert_eq!(drained.len(), 1, "应用只该入队一条请求");
    match &drained[0] {
        crate::ports::ResultRequest::ApplyFilterSettings {
            where_text,
            order_text,
        } => {
            assert_eq!(where_text, "amount > 1", "WHERE 取筛选框当前文本");
            assert_eq!(order_text, "id DESC", "ORDER 取浮层里的排序草稿");
        }
        other => panic!("应用应入队 ApplyFilterSettings，实际 {other:?}"),
    }
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(
        cx.debug_bounds("editor-result-filter-settings-card").is_none(),
        "应用之后浮层应关闭"
    );
}



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

/// 页签下标 → 枚举：0=列浏览器（默认）/ 1=值详情 / 2=计算 / 3=分组，越界回落默认页
#[test]
fn right_tab_index_maps_to_four_tabs() {
    assert_eq!(right_tab_from_index(0), RightTab::Browser);
    assert_eq!(right_tab_from_index(1), RightTab::Value);
    assert_eq!(right_tab_from_index(2), RightTab::Stats);
    assert_eq!(right_tab_from_index(3), RightTab::Group);
    assert_eq!(right_tab_from_index(99), RightTab::Browser);
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
            p.set_grids(grid, None, cx);
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
            p.set_grids(grid, None, cx);
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

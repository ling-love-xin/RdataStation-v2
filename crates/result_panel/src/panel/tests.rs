//! panel/ 全量单元测试

use super::next_right_edge_state;
use super::reorder_by;
use super::right_tab_from_index;
use super::sidebar_width_anim;
use super::tabs::stat_value;
use crate::column_browser::filter_browser_columns;
use crate::grid::stats::ColumnStats;
use crate::ports::{RightTab, StatsItem, StatsMode};
use gpui_kit::Pixels;
use gpui_kit::px;

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

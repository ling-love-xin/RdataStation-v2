//! panel/ 全量单元测试

use super::clamp_right_sidebar_height;
use super::clamp_right_sidebar_width;
use super::next_right_edge_state;
use super::reorder_by;
use super::right_tab_from_index;
use super::sidebar_width_anim;
use super::tabs::stat_value;
use super::tabs::StatExtras;
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

/// 【左缘竖条】视图按钮提示 = 「名字（描述）」，两条互不相同；记录视图的提示随开关
/// 状态说明会看到什么（对齐 `updatePresentationInToolbar:948-952` 的
/// `setText(label)` + `setToolTipText(description)`）。
#[test]
fn presentation_tooltips_carry_a_description() {
    use crate::panel::events::{presentation_tooltip, record_view_tooltip};
    use crate::ports::Presentation;

    let grid = presentation_tooltip(Presentation::Grid);
    let text = presentation_tooltip(Presentation::Text);
    for tip in [&grid, &text] {
        assert!(tip.contains('（') && tip.contains('）'), "缺描述：{tip}");
    }
    assert!(grid.starts_with("网格视图"), "{grid}");
    assert!(text.starts_with("文本视图"), "{text}");
    assert_ne!(grid, text);

    // 记录视图：未激活时报「会看到什么」，已激活时报「怎么回去」
    let off = record_view_tooltip(false);
    let on = record_view_tooltip(true);
    assert!(off.contains("转置"), "{off}");
    assert!(on.contains("网格"), "{on}");
    assert_ne!(off, on);
}

/// 【计算页右键菜单】条目清单 = 复制值 / 复制全部 / 分隔 / 缺项逐个「添加」/ 分隔 /
/// 恢复默认 / 分隔 / 口径切换；缺项为空时整段跳过（DBeaver 此时菜单为空、点了没反应）。
#[test]
fn stat_context_menu_mirrors_the_toolbar_actions() {
    use crate::panel::tabs::{StatMenuEntry, stat_menu_entries, stat_menu_label};

    // 只启用「非空」→ 其余八项逐个列出来
    let entries = stat_menu_entries(&[StatsItem::Count], StatsMode::Numeric);
    let labels: Vec<String> = entries.iter().map(stat_menu_label).collect();
    assert_eq!(labels[0], "复制值");
    assert_eq!(labels[1], "复制全部");
    assert!(matches!(entries[2], StatMenuEntry::Separator));
    let adds = entries
        .iter()
        .filter(|entry| matches!(entry, StatMenuEntry::AddMissing(_)))
        .count();
    assert_eq!(adds, 8, "九项里已启用一项 → 八个可添加");
    // 末尾是口径切换，且报「切到哪一档」（当前数值 → 报字符串）
    assert!(matches!(
        entries.last(),
        Some(StatMenuEntry::ToggleMode(StatsMode::Text))
    ));
    assert_eq!(
        stat_menu_label(entries.last().unwrap()),
        "改为按字符串聚合"
    );
    // 恢复默认仍在（DBeaver ResetFunctionsAction）
    assert!(entries.iter().any(|entry| matches!(entry, StatMenuEntry::Reset)));

    // 九项全启用 → 没有「添加」段，也就没有那条悬空分隔线
    let entries = stat_menu_entries(&StatsItem::ALL, StatsMode::Text);
    assert!(
        !entries
            .iter()
            .any(|entry| matches!(entry, StatMenuEntry::AddMissing(_))),
        "全启用时不应再列添加项"
    );
    let separators = entries
        .iter()
        .filter(|entry| matches!(entry, StatMenuEntry::Separator))
        .count();
    assert_eq!(separators, 2, "复制段后一条、恢复默认后一条");
    assert_eq!(
        stat_menu_label(entries.last().unwrap()),
        "改为按数值聚合"
    );
}

/// 【R2 计算页】单项取值：口径决定 Min/Max 的比较方式，Avg/Sum/Median 只在数值口径出值，
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
    let plain = StatExtras {
        text_extremes: None,
        distinct: None,
        median: None,
        mode: None,
    };
    assert_eq!(
        stat_value(StatsItem::Min, &stats, &plain, StatsMode::Numeric),
        "1"
    );
    assert_eq!(
        stat_value(StatsItem::Max, &stats, &plain, StatsMode::Numeric),
        "2,000"
    );
    // 字符串口径：极值走字典序，Avg/Sum/Median 不出值
    let textual = StatExtras {
        text_extremes: Some(&text),
        distinct: None,
        median: None,
        mode: None,
    };
    assert_eq!(
        stat_value(StatsItem::Min, &stats, &textual, StatsMode::Text),
        "apple"
    );
    assert_eq!(
        stat_value(StatsItem::Max, &stats, &textual, StatsMode::Text),
        "zebra"
    );
    assert_eq!(
        stat_value(StatsItem::Avg, &stats, &textual, StatsMode::Text),
        "—"
    );
    assert_eq!(
        stat_value(StatsItem::Sum, &stats, &textual, StatsMode::Text),
        "—"
    );
    assert_eq!(
        stat_value(StatsItem::Median, &stats, &textual, StatsMode::Text),
        "—"
    );
    // 计数类不受口径影响（字符串口径下仍是千分位整数）
    assert_eq!(
        stat_value(StatsItem::Count, &stats, &plain, StatsMode::Text),
        "1,234"
    );
}

/// 【R2 计算页·新增三项】去重计数 / 中位数 / 众数：有值走千分位或原文，缺样本一律 `—`
/// （与 Avg/Sum 同一缺省口径——界面不摆空行）
#[test]
fn stat_value_covers_distinct_median_and_mode() {
    let stats = ColumnStats::default();
    let extras = StatExtras {
        text_extremes: None,
        distinct: Some(42),
        median: Some(2.5),
        mode: Some("x".to_string()),
    };
    assert_eq!(
        stat_value(StatsItem::CountDistinct, &stats, &extras, StatsMode::Numeric),
        "42"
    );
    assert_eq!(
        stat_value(StatsItem::CountDistinct, &stats, &extras, StatsMode::Text),
        "42",
        "去重计数不看口径（两种口径都是「不同取值个数」）"
    );
    assert_eq!(
        stat_value(StatsItem::Median, &stats, &extras, StatsMode::Numeric),
        "2.5"
    );
    assert_eq!(
        stat_value(StatsItem::Mode, &stats, &extras, StatsMode::Text),
        "x",
        "众数两种口径都给值文本"
    );
    // 缺样本：一律 —
    let empty = StatExtras {
        text_extremes: None,
        distinct: None,
        median: None,
        mode: None,
    };
    for item in [StatsItem::CountDistinct, StatsItem::Median, StatsItem::Mode] {
        assert_eq!(
            stat_value(item, &stats, &empty, StatsMode::Numeric),
            "—",
            "{item:?} 没有样本应该给 —"
        );
    }
}

/// 【R2 计算页】可选全集 = 九项、展示顺序固定；**默认集**对齐 DBeaver `isDefault()` 打标
/// （count + countDistinct → 本仓「非空」+「去重计数」）；口径默认数值且可来回切
#[test]
fn stats_defaults_are_the_dbeaver_default_set_in_display_order() {
    assert_eq!(StatsItem::ALL.len(), 9);
    assert_eq!(StatsItem::ALL[0], StatsItem::Count);
    assert_eq!(StatsItem::ALL[1], StatsItem::CountDistinct);
    assert_eq!(StatsItem::ALL[8], StatsItem::Sum);
    // 默认集 = count + countdistinct（DBeaver 里也只有这两个标了 default）
    assert_eq!(
        StatsItem::DEFAULT_SET,
        [StatsItem::Count, StatsItem::CountDistinct]
    );
    assert!(StatsItem::Count.is_default());
    assert!(StatsItem::CountDistinct.is_default());
    assert!(!StatsItem::Nulls.is_default());
    assert!(!StatsItem::Sum.is_default());
    // 字符串口径：计数 / 极值 / 众数有意义；SUM/AVG/中位数要能排序的数字
    assert!(StatsItem::Min.supports_text_mode());
    assert!(StatsItem::CountDistinct.supports_text_mode());
    assert!(StatsItem::Mode.supports_text_mode());
    assert!(!StatsItem::Sum.supports_text_mode());
    assert!(!StatsItem::Median.supports_text_mode());
    // 只有这三项要扫整列取值（其余走列式统计）
    for item in StatsItem::ALL {
        let needs = matches!(
            item,
            StatsItem::CountDistinct | StatsItem::Median | StatsItem::Mode
        );
        assert_eq!(item.needs_column_values(), needs, "{item:?}");
    }
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
/// 都要真画出来；行高 20px、工具条 28px。
///
/// 六行是**手动置全**的（出厂默认只有「非空」，对齐 DBeaver 的 `isDefault()` 集合，
/// 见 §8.91）；同时钉住 Reset 的新语义：回到默认集，而不是六项全开。
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
            // 本用例要验「六行都画得出来」，出厂默认只有一项 → 先置全六项
            p.stats_items = crate::ports::StatsItem::ALL.to_vec();
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

    // 3) 行为闭环：停用 Min → 该行消失；Reset → 回到**默认集**（只剩「非空」），
    //    被停用的 Min 与其余非默认项都不回来（对齐 DBeaver `ResetFunctionsAction`
    //    = `loadDefaultFunctions()` 只收 `isDefault()`）
    cx.update(|_, app| {
        panel.update(app, |p, cx| {
            p.remove_stats_item(StatsItem::Min, cx);
        })
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(
        !visible("editor-result-stat-row-Min", cx),
        "停用 Min 后该行应消失"
    );
    cx.update(|_, app| panel.update(app, |p, cx| p.reset_stats_items(cx)));
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let after_reset = panel.read_with(cx, |p, _app| p.stats_items.clone());
    assert_eq!(
        after_reset,
        crate::ports::StatsItem::DEFAULT_SET.to_vec(),
        "Reset 应回到默认集（只有「非空」），不是六项全开"
    );
    assert!(
        visible("editor-result-stat-row-Count", cx),
        "Reset 后默认项「非空」行应在"
    );
    assert!(
        !visible("editor-result-stat-row-Sum", cx) && !visible("editor-result-stat-row-Min", cx),
        "Reset 后非默认项（Sum / Min）不应自动回来"
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

/// 【值页空态】形态对齐 DBeaver `ValueViewerPanel.createContents:89-106`：空态是**两行**
/// （选值提示 + 收起面板提示），且页内工具条仍在（六页同形）。
#[gpui_kit::test]
fn value_page_empty_state_shows_two_hint_lines_with_toolbar(
    cx: &mut gpui_kit::TestAppContext,
) {
    cx.update(gpui_kit::init);
    let (panel, cx) = panel_harness(cx);

    // 纯函数层：两行非空、互不相同（不是把一句话拆两截）
    let lines = {
        use super::tabs::value_empty_lines;
        value_empty_lines()
    };
    assert_eq!(lines.len(), 2, "值页空态应是两行");
    assert!(lines.iter().all(|line| !line.trim().is_empty()));
    assert_ne!(lines[0], lines[1]);
    // 第二行必须兑现「怎么收起面板」这条语义（本仓的两个入口之一）
    assert!(
        lines[1].contains("面板"),
        "第二行应指向收起面板的入口，实际 {}",
        lines[1]
    );

    // 渲染层：未选中单元格 → 空态页（带工具条）+ 新空态容器可见
    cx.update(|_, app| {
        panel.update(app, |p, cx| {
            p.selection_pos = None;
            p.set_right_tab(RightTab::Value, cx);
        })
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(
        cx.debug_bounds("editor-result-value-toolbar").is_some(),
        "值页空态也要保留页内工具条（六页同形）"
    );
    assert!(
        cx.debug_bounds("editor-result-empty-page").is_some(),
        "值页空态正文容器应渲染（editor-result-empty-page）"
    );
}

/// 【分组页持久化】宿主注入的两项偏好：排序口径 id 与「仅重复」开关都要落到面板状态；
/// 未知 id 按出厂 `value`（与设置服务归一同一口径），重复注入同值不重复 notify。
#[gpui_kit::test]
fn group_prefs_initial_round_trip_and_unknown_sort_falls_back(
    cx: &mut gpui_kit::TestAppContext,
) {
    cx.update(gpui_kit::init);
    let (panel, cx) = panel_harness(cx);

    // 面板出厂态：按值 + 不只显示重复
    assert_eq!(
        cx.update(|_, app| panel.read(app).group_sort_id()),
        "value"
    );
    assert!(!cx.update(|_, app| panel.read(app).group_duplicates_only_value()));

    // 注入用户上次的口径：按计数降序 + 仅重复
    cx.update(|_, app| {
        panel.update(app, |p, cx| {
            p.set_group_prefs_initial("countdesc", true, cx);
        })
    });
    assert_eq!(
        cx.update(|_, app| panel.read(app).group_sort_id()),
        "countdesc"
    );
    assert!(cx.update(|_, app| panel.read(app).group_duplicates_only_value()));

    // 未知 id（手改配置文件）→ 回落到出厂 value，开关照注入（两字段各自独立）
    cx.update(|_, app| {
        panel.update(app, |p, cx| {
            p.set_group_prefs_initial("asc", false, cx);
        })
    });
    assert_eq!(
        cx.update(|_, app| panel.read(app).group_sort_id()),
        "value",
        "未知排序 id 应按出厂 value 处理（不猜 ASC 之类的档位）"
    );
    assert!(!cx.update(|_, app| panel.read(app).group_duplicates_only_value()));
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

/// 【工具栏配置】状态栏命令项的默认可见性对齐 DBeaver `plugin.xml:1381-1397`：
/// `undo` / `redo` 两项 `defaultVisibility="false"`（取下一段是有意偏离，常显），
/// 其余 `true`；配置菜单把它们勾回来后应真的重新出现（否则隐藏 = 永久失去入口）。
#[gpui_kit::test]
fn status_commands_follow_dbeaver_default_visibility_and_can_be_restored(
    cx: &mut gpui_kit::TestAppContext,
) {
    use gpui_kit::test::TestWindowExt as _;
    use gpui_kit::{ElementId, SharedString};

    cx.update(gpui_kit::init);
    let (panel, cx) = panel_harness(cx);
    // 有状态行才谈得上状态行的命令项（`show_status` 需要 status / 执行信息之一）
    cx.update(|_window, app| {
        panel.update(app, |p, cx| {
            p.status = Some(crate::grid::status::ResultStatus {
                total_rows: 2,
                ..Default::default()
            });
            cx.notify();
        })
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));

    let visible = |id: &'static str, cx: &mut gpui_kit::VisualTestContext| -> bool {
        let by_id = cx.update(|window, _| {
            window
                .try_find(ElementId::Name(SharedString::from(id.to_string())))
                .is_some_and(|snap| snap.visible())
        });
        by_id || cx.debug_bounds(id).is_some()
    };

    // 1) 默认隐藏集合 = 两项（撤销 / 重做；取下一段有意常显）
    let hidden = panel.read_with(cx, |p, _| p.hidden_status_commands().clone());
    assert_eq!(
        hidden.len(),
        2,
        "默认隐藏只该有两项（undo / redo）：{hidden:?}"
    );
    assert!(hidden.contains(&crate::ports::StatusCommand::Undo));
    assert!(hidden.contains(&crate::ports::StatusCommand::Redo));
    assert!(
        !hidden.contains(&crate::ports::StatusCommand::FetchPage),
        "取下一段常显（本仓唯一取数入口）"
    );

    // 2) 状态行里撤销 / 重做不摆（注入钮 id 是 `editor-result-undo` / `-redo`，
    //    未注入时回退占位 `editor-result-action-Undo2`），首页这类默认可见的还在
    assert!(
        !visible("editor-result-undo", cx) && !visible("editor-result-action-Undo2", cx),
        "撤销默认隐藏（DBeaver defaultVisibility=false）"
    );
    assert!(
        !visible("editor-result-redo", cx) && !visible("editor-result-action-Redo2", cx),
        "重做默认隐藏"
    );
    assert!(
        visible("editor-result-page-first", cx),
        "首页默认显示（注入钮 id 是 `editor-result-page-first`）"
    );

    // 3) 配置菜单本身挂出来了
    assert!(
        visible("editor-result-status-config", cx),
        "状态行末端要有工具栏配置菜单入口"
    );

    // 4) 勾回撤销：状态行重新摆出撤销钮
    cx.update(|_window, app| {
        panel.update(app, |p, cx| {
            p.set_status_command_visible(crate::ports::StatusCommand::Undo, true, cx)
        })
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(
        visible("editor-result-undo", cx),
        "勾选后撤销钮应回到状态行"
    );
    assert_eq!(
        panel.read_with(cx, |p, _| p.hidden_status_commands().len()),
        1,
        "勾选只移除该项（原两项剩一项）"
    );

    // 5) 反向：隐藏一个默认可见的项
    cx.update(|_window, app| {
        panel.update(app, |p, cx| {
            p.set_status_command_visible(crate::ports::StatusCommand::First, false, cx)
        })
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(
        !visible("editor-result-page-first", cx),
        "取消勾选后首页钮应消失"
    );
}

/// 【工具栏配置落盘回读】宿主按全局偏好（`result_panel.hidden_status_commands`）
/// 在创建面板时整批注入隐藏集合：面板必须以注入值为**唯一**权威（整批替换，不与
/// 出厂默认合并），否则用户在 ▾ 菜单里勾回来的项会在下次启动时被默认集合吃掉。
#[gpui_kit::test]
fn injected_hidden_status_commands_replace_the_default_set(cx: &mut gpui_kit::TestAppContext) {
    use gpui_kit::test::TestWindowExt as _;
    use gpui_kit::{ElementId, SharedString};

    cx.update(gpui_kit::init);
    let (panel, cx) = panel_harness(cx);
    cx.update(|_window, app| {
        panel.update(app, |p, cx| {
            p.status = Some(crate::grid::status::ResultStatus {
                total_rows: 2,
                ..Default::default()
            });
            // 偏好文件里写的是「undo,first」：撤销（出厂就隐藏）+ 首页（出厂显示，用户手动勾掉）
            p.set_hidden_status_commands(
                crate::ports::StatusCommand::parse_hidden_list("undo,first"),
                cx,
            );
        })
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));

    let hidden = panel.read_with(cx, |p, _| p.hidden_status_commands().clone());
    assert_eq!(
        hidden.len(),
        2,
        "整批替换：出厂默认里的「重做」应回到显示，只剩注入的两项：{hidden:?}"
    );
    assert!(hidden.contains(&crate::ports::StatusCommand::Undo));
    assert!(hidden.contains(&crate::ports::StatusCommand::First));
    assert!(!hidden.contains(&crate::ports::StatusCommand::Redo));

    let visible = |id: &'static str, cx: &mut gpui_kit::VisualTestContext| -> bool {
        let by_id = cx.update(|window, _| {
            window
                .try_find(ElementId::Name(SharedString::from(id.to_string())))
                .is_some_and(|snap| snap.visible())
        });
        by_id || cx.debug_bounds(id).is_some()
    };
    assert!(
        !visible("editor-result-page-first", cx),
        "注入里含首页 → 首页钮不该摆出来"
    );
    assert!(
        visible("editor-result-redo", cx),
        "注入里没有重做 → 重做钮应出现（不被出厂默认吃掉）"
    );
}

/// 【Metadata · 生成 DDL】列浏览器页内工具条的「生成 DDL」钮（对齐 DBeaver
/// `MetaDataPanel.contributeActions` 的 `generate.ddl.by.resultSet`）：常驻可见，
/// 点它入队 `GenerateResultSetDdl`（文本生成与预览浮层在宿主分派后由面板完成）。
#[gpui_kit::test]
fn column_browser_toolbar_queues_the_generate_ddl_request(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let (panel, vcx) = panel_harness(cx);
    vcx.update(|window, cx| window.draw(cx).clear(cx));

    let button = vcx
        .debug_bounds("editor-result-generate-ddl")
        .expect("列浏览器页内工具条应有「生成 DDL」钮（DBeaver metadata 面板唯一动作）");
    let point = gpui_kit::point(
        button.origin.x + button.size.width / 2.,
        button.origin.y + button.size.height / 2.,
    );
    vcx.simulate_click(point, gpui_kit::Modifiers::default());
    vcx.update(|window, cx| window.draw(cx).clear(cx));

    let queued = vcx.update(|_, app| panel.read(app).queue.drain());
    assert!(
        queued
            .iter()
            .any(|request| matches!(request, crate::ports::ResultRequest::GenerateResultSetDdl)),
        "点「生成 DDL」应入队 GenerateResultSetDdl：{queued:?}，按钮 {button:?}"
    );
}

/// 【Metadata · 生成 DDL】生成结果走 SQL 预览浮层：有驱动类型时出 `CREATE TABLE`，
/// 类型全缺（认不出值族）时给说明而不是编类型（对齐 DBeaver 的
/// `sql_generator_nonsql_text` 分支）。
#[gpui_kit::test]
fn generate_ddl_dialog_shows_sql_or_an_explanation(cx: &mut gpui_kit::TestAppContext) {
    use crate::store::ColumnType;

    cx.update(gpui_kit::init);
    let (panel, vcx) = panel_harness(cx);

    // 1) 两列都补驱动类型 → 出 SQL
    vcx.update(|_window, app| {
        panel.update(app, |p, cx| {
            p.grid.update(cx, |grid, cx| {
                grid.delegate_mut().set_column_types(vec![
                    Some(ColumnType::parse("integer")),
                    Some(ColumnType::parse("numeric(10,2)")),
                ]);
                cx.notify();
            });
        });
    });
    vcx.update(|window, app| {
        panel.update(app, |p, cx| p.open_ddl_dialog(window, cx));
    });
    vcx.update(|window, cx| window.draw(cx).clear(cx));

    let (open, title) = vcx.update(|_, app| {
        let p = panel.read(app);
        (p.sql_dialog_open, p.sql_dialog_title.clone())
    });
    assert!(open, "生成 DDL 应打开预览浮层");
    assert_eq!(
        title.map(|title| title.to_string()).as_deref(),
        Some("生成 DDL（按结果集列建表）"),
        "浮层标题要说明这是生成 DDL（同一浮层也复用给原始 SQL 预览）"
    );

    // 2) 清掉类型 → 两列都是 Unknown 值族，不能编类型出来 → 浮层里应是说明句
    vcx.update(|_window, app| {
        panel.update(app, |p, cx| {
            p.grid.update(cx, |grid, cx| {
                grid.delegate_mut().set_column_types(vec![None, None]);
                cx.notify();
            });
        });
    });
    vcx.update(|window, app| {
        panel.update(app, |p, cx| p.open_ddl_dialog(window, cx));
    });
    // 关闭后标题复位（原始 SQL 预览不能顶着「生成 DDL」的标题）
    vcx.update(|_window, app| panel.update(app, |p, cx| p.close_sql_dialog(cx)));
    let title = vcx.update(|_, app| panel.read(app).sql_dialog_title.clone());
    assert!(title.is_none(), "关掉浮层后标题要复位成「原始 SQL」档");
}

/// 【结果集设置】状态行最右端的齿轮（对齐 DBeaver 状态栏 `ConfigAction`，
/// `ResultSetViewer.java:5402-5418`）：有状态行时必摆，点它只推
/// `OpenResultPanelSettings`（面板不认识 settings crate，由宿主派发打开设置的动作）。
#[gpui_kit::test]
fn status_settings_gear_opens_the_result_panel_preferences(
    cx: &mut gpui_kit::TestAppContext,
) {
    cx.update(gpui_kit::init);
    let (panel, cx) = panel_harness(cx);
    cx.update(|_window, app| {
        panel.update(app, |p, cx| {
            p.status = Some(crate::grid::status::ResultStatus {
                total_rows: 2,
                ..Default::default()
            });
            cx.notify();
        })
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));

    let gear = cx
        .debug_bounds("editor-result-status-preferences")
        .expect("状态行最右端应有「结果集设置」齿轮（DBeaver 的 ConfigAction）");
    let point = gpui_kit::point(
        gear.origin.x + gear.size.width / 2.,
        gear.origin.y + gear.size.height / 2.,
    );
    cx.simulate_click(point, gpui_kit::Modifiers::default());
    cx.update(|window, cx| window.draw(cx).clear(cx));

    let queued = cx.update(|_, app| panel.read(app).queue.drain());
    assert!(
        queued
            .iter()
            .any(|request| matches!(request, crate::ports::ResultRequest::OpenResultPanelSettings)),
        "点齿轮应入队「打开结果集设置」：{queued:?}，齿轮 {gear:?}"
    );
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

/// 【复制列名】剪贴板文本：多选集合逐行拼（换行分隔）、单列只拷自己、越界跳过。
///
/// 对齐 DBeaver `MetaDataPanel.fillCustomActions` 的 `meta_data_panel_action_copy_column_text`
/// （选中几行就拷几行，`"\n"` 分隔，空文本直接返回不写剪贴板）。
#[test]
fn copy_column_names_joins_the_selection_with_newlines() {
    use crate::column_browser::column_names_to_copy;

    let columns = vec![
        "id".to_string(),
        "name".to_string(),
        "amount".to_string(),
    ];

    // 单选：右键那一列不在集合里（或集合是空）→ 只拷它自己
    assert_eq!(column_names_to_copy(&columns, &[], 1), "name");
    assert_eq!(column_names_to_copy(&columns, &[0], 2), "amount");

    // 多选且右键的那一列在集合里 → 整个集合按数据列序（换行分隔）
    assert_eq!(
        column_names_to_copy(&columns, &[0, 2], 2),
        "id\namount",
        "选中两列时一次带走两行"
    );

    // 越界下标跳过（搜索过滤后旧下标可能失效）：集合里只剩有效的那几个
    assert_eq!(
        column_names_to_copy(&columns, &[0, 9], 9),
        "id",
        "9 在集合里但列已不存在 → 跳过它，只留有效列"
    );
    assert_eq!(
        column_names_to_copy(&columns, &[87, 99], 99),
        "",
        "集合全越界 = 空文本（调用方据此不写剪贴板）"
    );
    // 右键那一列越界且不在集合里 → 同样空文本
    assert_eq!(column_names_to_copy(&columns, &[0], 9), "");
    // 列名清单本身为空（无列的结果集）→ 空文本，不 panic
    assert_eq!(column_names_to_copy(&[], &[0, 1], 0), "");
}

/// 【列注释】清单第二行的取值：`None` / 空串 / 纯空白都不占行（DBeaver 的
/// `Description` 额外列对没有注释的列显示空白），有内容则去掉首尾空白后原样显示。
#[test]
fn browser_comment_skips_empty_and_blank() {
    use crate::column_browser::browser_comment_text;

    assert_eq!(browser_comment_text(&None), None);
    assert_eq!(browser_comment_text(&Some(String::new())), None);
    assert_eq!(browser_comment_text(&Some("   ".to_string())), None);
    assert_eq!(
        browser_comment_text(&Some("  用户登录名 ".to_string())),
        Some("用户登录名".to_string()),
        "注释去首尾空白后原样显示"
    );
}

/// 【列注释】渲染路径：有注释的列在列名下画出第二行，没有注释的列不画。
/// 数据经 delegate 投影直灌（`sync` 的接线由 toolbar 既有用例覆盖，这里只验渲染）。
/// 【列浏览器 · MetaDataPanel.refresh 短路】投影完全相同就什么都不做：
/// 对齐 `MetaDataPanel.refresh:184-196`（属性清单逐项相同 → 直接 return，注释 "No changes"）。
/// 本仓 `set_data` 会清 `last_selected` 与多选集合，若「刷新到同一份结果」也照清，
/// 用户的多选会被无意义地打掉。
#[gpui_kit::test]
fn identical_column_projection_keeps_the_browser_selection(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let (panel, cx) = panel_harness(cx);

    let cols = || vec!["id".to_string(), "amount".to_string()];
    let types = || vec!["INTEGER".to_string(), "DECIMAL".to_string()];
    let hidden = || vec![false, false];
    let comments = || vec![Some("主键".to_string()), None];

    cx.update(|_, app| {
        panel.update(app, |p, cx| {
            let list = p.browser_list.clone();
            list.update(cx, |list, cx| {
                list.delegate_mut()
                    .set_data(cols(), types(), hidden(), comments());
                list.delegate_mut().set_selected_columns(vec![0, 1]);
                assert_eq!(list.delegate().selected_columns(), &[0, 1]);
                // 同一份投影再来一次 → 选集保留（不清空）
                list.delegate_mut()
                    .set_data(cols(), types(), hidden(), comments());
                assert_eq!(
                    list.delegate().selected_columns(),
                    &[0, 1],
                    "投影没变时不该把用户的多选打掉"
                );
                // 换成另一份投影（列数变了）→ 旧序号失效，选集清空
                list.delegate_mut().set_data(
                    vec!["id".to_string()],
                    vec!["INTEGER".to_string()],
                    vec![false],
                    vec![None],
                );
                assert!(
                    list.delegate().selected_columns().is_empty(),
                    "换结果集后旧列序号必须失效"
                );
                cx.notify();
            });
        });
    });
}

/// 【列浏览器 · 页内工具条】DBeaver `MetaDataPanel.contributeActions:204-215` 只贡献
/// 一条「按结果集生成 DDL」（`generate.ddl.by.resultSet`，恒可用）；本仓列浏览器页
/// 工具条常驻的第一颗动作就是它——这里钉住入口存在。
#[gpui_kit::test]
fn column_browser_toolbar_offers_generate_ddl(cx: &mut gpui_kit::TestAppContext) {

    cx.update(gpui_kit::init);
    // 默认页就是列浏览器，面板实体本身不再用到（只要它被挂上窗口即可）
    let (_panel, cx) = panel_harness(cx);

    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(
        cx.debug_bounds("editor-result-generate-ddl").is_some(),
        "列浏览器页工具条应常驻「生成 DDL」入口"
    );
}

/// 【计算行数】可点性 = 有数据 且 不是本地分析结果；提示分三态（分析结果讲原因、
/// 已算过讲「重算」、没算过讲「算什么」）。分析结果两侧（面板置灰 + 宿主兜底）
/// 同一条口径：临时表早就不在了，重跑原 SQL 只会报错。
#[test]
fn count_rows_action_is_gated_by_data_and_analysis_tier() {
    use crate::panel::events::{count_rows_available, count_rows_tooltip};

    assert!(count_rows_available(true, false), "有数据且普通结果 → 可点");
    assert!(!count_rows_available(false, false), "没有数据 → 置灰（DBeaver hasData）");
    assert!(
        !count_rows_available(true, true),
        "本地分析结果 → 置灰（临时表不在，重跑原 SQL 必错）"
    );

    let analysis = count_rows_tooltip(true, false);
    assert!(
        analysis.contains("分析结果") && analysis.contains("没有可重算的源查询"),
        "分析结果的提示要说清为什么不能算：{analysis}"
    );
    assert_eq!(
        count_rows_tooltip(false, true),
        "总行数（对原 SQL 跑 count(*)）：点一下重新计算"
    );
    assert_eq!(
        count_rows_tooltip(false, false),
        "计算行数（对原 SQL 跑 count(*)，不产生新结果集）"
    );
}

#[gpui_kit::test]
fn status_bar_offers_a_count_rows_action(cx: &mut gpui_kit::TestAppContext) {

    cx.update(gpui_kit::init);
    let (panel, cx) = panel_harness(cx);
    // 状态行只在有 `status`（或执行信息）时才摆：测试壳默认没注入，这里补一条
    cx.update(|_window, app| {
        panel.update(app, |p, cx| {
            p.status = Some(crate::grid::status::ResultStatus {
                total_rows: 2,
                ..Default::default()
            });
            cx.notify();
        })
    });

    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(
        cx.debug_bounds("editor-result-count-rows").is_some(),
        "状态行应摆「计算行数」钮（对齐 DBeaver `rowCountLabel`）"
    );

    // 宿主算回来的数字**就地**变成钮的文案（不开新结果集、不增标签页）
    cx.update(|_, app| {
        panel.update(app, |p, cx| {
            p.set_row_count_notice("总行数 1,234".to_string(), cx)
        });
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(
        cx.debug_bounds("editor-result-count-rows").is_some(),
        "回填总行数后同一颗钮仍在"
    );
}

#[gpui_kit::test]
fn column_browser_draws_the_comment_line_below_the_name(cx: &mut gpui_kit::TestAppContext) {

    cx.update(gpui_kit::init);
    let (panel, cx) = panel_harness(cx);
    // 只有第一列（id）有注释，第二列（amount）没有 → 第二行应该只出现一次
    cx.update(|_, app| {
        panel.update(app, |p, cx| {
            let list = p.browser_list.clone();
            list.update(cx, |list, cx| {
                list.delegate_mut().set_data(
                    vec!["id".to_string(), "amount".to_string()],
                    vec!["INTEGER".to_string(), "DECIMAL".to_string()],
                    vec![false, false],
                    vec![Some("主键".to_string()), None],
                );
                cx.notify();
            });
        });
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));

    let visible = |id: &'static str, cx: &mut gpui_kit::VisualTestContext| -> bool {
        cx.debug_bounds(id).is_some()
    };
    assert!(
        visible("editor-result-col-browser-comment", cx),
        "有注释的列应在列名下画出注释行"
    );

    // 反证：全部列都没有注释时，第二行不出现（不许画一行空文本占高）
    cx.update(|_, app| {
        panel.update(app, |p, cx| {
            let list = p.browser_list.clone();
            list.update(cx, |list, cx| {
                list.delegate_mut().set_data(
                    vec!["id".to_string(), "amount".to_string()],
                    vec!["INTEGER".to_string(), "DECIMAL".to_string()],
                    vec![false, false],
                    vec![None, None],
                );
                cx.notify();
            });
        });
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(
        !visible("editor-result-col-browser-comment", cx),
        "没有注释的列不应占第二行"
    );
}

/// 【R5 面板最大化】对齐 DBeaver `CMD_TOGGLE_MAXIMIZE` → `togglePanelsMaximize`
/// （`ResultSetViewer.java:1551-1557` = `viewerSash.hideUp()/showUp()`）：
/// 最大化时**网格整块不渲染**，右栏独占区域；再切回网格回来。
#[gpui_kit::test]
fn maximize_panel_hides_the_grid_and_keeps_the_sidebar(cx: &mut gpui_kit::TestAppContext) {

    cx.update(gpui_kit::init);
    let (panel, cx) = panel_harness(cx);

    // 基线：网格在、右栏在
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(
        cx.debug_bounds("editor-result-main-grid").is_some(),
        "基线应渲染结果网格"
    );
    assert!(
        cx.debug_bounds("editor-result-value-sidebar").is_some(),
        "基线应渲染右栏"
    );

    // 最大化：网格整块消失，右栏仍在（独占区域）
    cx.update(|_, app| panel.update(app, |p, cx| p.toggle_panel_maximize(cx)));
    assert!(
        cx.update(|_, app| panel.read(app).panel_maximized),
        "最大化标记应翻转"
    );
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(
        cx.debug_bounds("editor-result-main-grid").is_none(),
        "最大化时网格不应再渲染"
    );
    assert!(
        cx.debug_bounds("editor-result-value-sidebar").is_some(),
        "最大化时右栏必须还在"
    );

    // 还原：网格回来，开合态（`show_value_sidebar`）不被最大化改动
    cx.update(|_, app| panel.update(app, |p, cx| p.toggle_panel_maximize(cx)));
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(
        cx.debug_bounds("editor-result-main-grid").is_some(),
        "还原后网格应重新渲染"
    );
    assert!(
        cx.update(|_, app| panel.read(app).show_value_sidebar),
        "最大化不应改动右栏开合态"
    );
}

/// 【右缘竖条】提示文案 = 页名 + 面板描述（对齐 `ResultSetViewer.java:1055-1080`
/// 的 `label + " (" + toolTip + ")"`）；该页正在显示时改报「点它会收起」。
#[test]
fn edge_bar_tooltip_carries_label_and_description() {
    use super::events::{edge_tab_tooltip, right_tab_description, right_tab_label};
    use crate::ports::RightTab;

    for tab in super::events::right_tabs() {
        let tip = edge_tab_tooltip(tab, false);
        assert!(tip.contains(right_tab_label(tab)), "{} 的提示应含页名", tip);
        assert!(
            tip.contains(right_tab_description(tab)),
            "{} 的提示应含描述",
            tip
        );
        assert!(
            !right_tab_description(tab).is_empty(),
            "{} 的描述不应为空",
            right_tab_label(tab)
        );
    }

    // 正在显示的页：提示改说「收起」，不再堆描述
    let collapse_tip = edge_tab_tooltip(RightTab::Value, true);
    assert!(collapse_tip.contains("收起"));
    assert!(
        !collapse_tip.contains(right_tab_description(RightTab::Value)),
        "收起提示不必再塞面板描述"
    );
}

/// 【右缘竖条 / 视图菜单】点某一页的开关语义（纯函数）：对齐 DBeaver
/// `ResultSetHandlerTogglePanel.toggleResultsPanel:64-69` + `showResultsPanel:71-81`
/// （`isVisible = isPanelsVisible() && isPanelVisible(id)`，真则 `closePanel`）。
#[test]
fn right_tab_click_closes_only_the_page_that_is_showing() {
    use super::events::right_tab_click_collapses;
    use crate::ports::RightTab;

    // 面板可见 + 点的正是当前页 → 收起
    assert!(right_tab_click_collapses(
        RightTab::Value,
        RightTab::Value,
        true
    ));
    // 面板可见但点的是别的页 → 切页（不收起）
    assert!(!right_tab_click_collapses(
        RightTab::Stats,
        RightTab::Value,
        true
    ));
    // 面板不可见 → 任何点击都是「展开并切到该页」
    assert!(!right_tab_click_collapses(
        RightTab::Value,
        RightTab::Value,
        false
    ));
}

/// 同上的状态机行为（窗口用例）：连点两次同一页 = 收起 → 再点又回到该页。
#[gpui_kit::test]
fn toggling_the_active_page_collapses_the_panel(cx: &mut gpui_kit::TestAppContext) {
    use crate::ports::RightTab;

    cx.update(gpui_kit::init);
    let (panel, cx) = panel_harness(cx);

    cx.update(|_, app| panel.update(app, |p, cx| p.set_right_tab(RightTab::Value, cx)));
    assert!(cx.update(|_, app| panel.read(app).panels_visible()));

    // 已显示 → 再点这一页 = 收起
    let switched = cx.update(|_, app| panel.update(app, |p, cx| p.toggle_right_tab(RightTab::Value, cx)));
    assert!(!switched, "点当前页不应算「切页」");
    assert!(
        !cx.update(|_, app| panel.read(app).panels_visible()),
        "点当前页应收起面板"
    );

    // 收起后再点 → 展开并切到该页
    let switched = cx.update(|_, app| panel.update(app, |p, cx| p.toggle_right_tab(RightTab::Value, cx)));
    assert!(switched, "收起态点击应是切页/展开");
    assert!(cx.update(|_, app| panel.read(app).panels_visible()));
}

/// 【R5 面板最大化】与「面板总开关」的互斥口径（不变量：最大化 ⇒ 面板展开）。
///
/// 取证依据：DBeaver 里两者共用同一份 sash 可见性——`togglePanelsMaximize` 是
/// `viewerSash.hideUp()/showUp()`（`ResultSetViewer.java:1551-1557`），而
/// `showPanels(false)` 就是 `viewerSash.setMaximizedControl(children[0])`（`:1487`），
/// 也就是「关面板」之后不可能还停在最大化。本仓若不同步复位，会出现
/// 「面板已关（竖条也没了）但右栏被最大化强行挂满整块」的自相矛盾。
#[gpui_kit::test]
fn maximize_yields_to_the_panels_master_switch(cx: &mut gpui_kit::TestAppContext) {

    cx.update(gpui_kit::init);
    let (panel, cx) = panel_harness(cx);

    // 1) 面板关着时点最大化 → 先把面板展开，再进入最大化（否则整块区域空白）
    cx.update(|_, app| panel.update(app, |p, cx| p.set_panels_visible(false, cx)));
    assert!(!cx.update(|_, app| panel.read(app).panels_visible()), "基线：面板已关");
    cx.update(|_, app| panel.update(app, |p, cx| p.toggle_panel_maximize(cx)));
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(
        cx.update(|_, app| panel.read(app).panels_visible()),
        "进入最大化必须隐含展开面板"
    );
    assert!(
        cx.update(|_, app| panel.read(app).panel_maximized),
        "最大化标记应已置位"
    );
    assert!(
        cx.debug_bounds("editor-result-value-sidebar").is_some(),
        "最大化时右栏必须在（否则网格收起后整块空白）"
    );
    assert!(
        cx.debug_bounds("editor-result-main-grid").is_none(),
        "最大化时网格不渲染"
    );

    // 2) 最大化期间关面板 → 最大化被吃掉（不复位就会留下"面板关了但右栏还占满"）
    cx.update(|_, app| panel.update(app, |p, cx| p.set_panels_visible(false, cx)));
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(
        !cx.update(|_, app| panel.read(app).panel_maximized),
        "关面板必须一并退出最大化（DBeaver 里两者是同一份 sash 可见性）"
    );
    assert!(
        !cx.update(|_, app| panel.read(app).panels_visible()),
        "关面板后「面板可见」应为假"
    );
    assert!(
        cx.debug_bounds("editor-result-main-grid").is_some(),
        "关面板后网格必须回到整块区域"
    );

    // 3) 最大化期间走状态栏三态开关到「收起」→ 同样复位最大化
    cx.update(|_, app| panel.update(app, |p, cx| p.set_panels_visible(true, cx)));
    cx.update(|_, app| panel.update(app, |p, cx| p.toggle_panel_maximize(cx)));
    assert!(cx.update(|_, app| panel.read(app).panel_maximized));
    // 宽展开（edge+show）→ 收起（edge/show 都关）
    cx.update(|_, app| panel.update(app, |p, cx| p.toggle_value_sidebar(cx)));
    assert!(
        !cx.update(|_, app| panel.read(app).show_value_sidebar),
        "三态开关这一跳应到「收起」"
    );
    assert!(
        !cx.update(|_, app| panel.read(app).panel_maximized),
        "三态开关走到收起时必须一并退出最大化"
    );
}

/// 【R3 分组页】「移除分组列」的可用性（纯函数）：对齐 DBeaver
/// `GroupingColumnsContainer.canColumnBeRemoved`（`:92-94`）——选中列在分组键里、
/// 且移除后仍至少剩一列才可用；删到零归「清空分组」管。
#[test]
fn remove_group_column_guards_the_last_key() {
    use super::tabs::can_remove_group_column;

    // 没有显式分组键（走选中列兜底）→ 没得删
    assert!(!can_remove_group_column(&[], Some(0)));
    // 只剩一列 → 不允许逐列删（哪怕选中的就是它）
    assert!(!can_remove_group_column(&[0], Some(0)));
    // 选中列在集合里且还有别的列 → 可删
    assert!(can_remove_group_column(&[0, 2], Some(2)));
    assert!(can_remove_group_column(&[0, 2], Some(0)));
    // 选中列不在集合里 / 没有选中列 → 不适用
    assert!(!can_remove_group_column(&[0, 2], Some(1)));
    assert!(!can_remove_group_column(&[0, 2], None));
}

/// 【R3 分组页】工具条形状：分组列下拉 / 排序口径 / 仅重复 / 移除分组列 / 清空分组
/// 五个入口都在，且顺序与分隔照 `GroupingPanel.fillToolBar:164-176`
/// （`Sort` → `Dups` → 分隔 → `EditColumns`（本仓=勾选下拉） → `DeleteColumn` → 分隔 → `Clear`）；
/// 排序口径的语义差异见 §8.83。移除钮的可用性由纯函数钉，这里只验它真的画出来。
#[gpui_kit::test]
fn group_toolbar_offers_the_remove_column_action(cx: &mut gpui_kit::TestAppContext) {

    cx.update(gpui_kit::init);
    let (panel, cx) = panel_harness(cx);
    cx.update(|_, app| {
        panel.update(app, |p, cx| {
            p.toggle_group_column(0, cx);
            p.toggle_group_column(2, cx);
            p.set_right_tab(RightTab::Group, cx);
        })
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));

    let visible = |id: &'static str, cx: &mut gpui_kit::VisualTestContext| -> bool {
        cx.debug_bounds(id).is_some()
    };
    for id in [
        "editor-result-group-columns",
        "editor-result-group-sort",
        "editor-result-group-dup",
        "editor-result-group-remove",
        "editor-result-group-clear",
    ] {
        assert!(visible(id, cx), "分组页工具条缺少入口：{id}");
    }
}

/// 【R6 段大小】输入框文本解析（纯函数）：对齐 DBeaver `resultSetSize` 的 modify 回调
/// （`ResultSetViewer.java:2138-2152`）——`>0` 才写，非数字 / 0 / 负数都不算有效提交。
#[test]
fn segment_rows_parse_accepts_positive_only() {
    use super::events::parse_segment_rows;

    assert_eq!(parse_segment_rows("200"), Some(200));
    assert_eq!(parse_segment_rows(" 1000 "), Some(1000), "首尾空白忽略");
    assert_eq!(parse_segment_rows("1"), Some(1));
    assert_eq!(parse_segment_rows("0"), None, "0 无效（DBeaver 只在 >0 时写偏好）");
    assert_eq!(parse_segment_rows("-5"), None, "负数无效");
    assert_eq!(parse_segment_rows("abc"), None, "非数字无效");
    assert_eq!(parse_segment_rows(""), None, "空串无效");
    assert_eq!(parse_segment_rows("12.5"), None, "小数不算行数");
}

/// 【R6 段大小】失焦回填（纯函数）：对齐 DBeaver `focusLost`（`ResultSetViewer.java:2129-2137`）
/// ——文本框内容与生效值不同就改写成生效值；相同则什么都不做（避免无谓改写光标）。
///
/// headless 里 `InputEvent::Blur` 不 emit（元素级焦点事件不派发，见
/// `crates/editor/src/view/tests/analysis.rs` 的同款注记），所以回填判定钉在这里。
#[test]
fn segment_rows_blur_restores_the_effective_value() {
    use super::events::segment_rows_blur_display;

    // 非法文本（敲了没回车 / 敲了 0）：回填成当前生效值
    assert_eq!(segment_rows_blur_display("abc", 1000), Some("1000".to_string()));
    assert_eq!(segment_rows_blur_display("0", 1000), Some("1000".to_string()));
    assert_eq!(segment_rows_blur_display("", 1000), Some("1000".to_string()));
    assert_eq!(segment_rows_blur_display("-5", 1000), Some("1000".to_string()));
    // 未提交的合法值（输入框还没来得及规范化）：同样按生效值收口
    assert_eq!(segment_rows_blur_display("200", 1000), Some("1000".to_string()));
    // 已是生效值（含用户敲的首尾空白）：不动
    assert_eq!(segment_rows_blur_display("1000", 1000), None);
    assert_eq!(segment_rows_blur_display(" 1000 ", 1000), None);
}

/// 【R6 段大小】面板状态：改值生效、同值不重入、下限夹取（`MIN_SEGMENT_SIZE = 1`）。
#[gpui_kit::test]
fn set_segment_rows_clamps_to_the_minimum(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let (panel, cx) = panel_harness(cx);

    assert_eq!(
        cx.update(|_, app| panel.read(app).segment_rows()),
        crate::ui::RESULT_SEGMENT_ROWS_DEFAULT,
        "默认一段 = 1000 行（与编辑器 SEGMENT_ROWS 同值）"
    );

    cx.update(|_, app| panel.update(app, |p, cx| p.set_segment_rows(500, cx)));
    assert_eq!(cx.update(|_, app| panel.read(app).segment_rows()), 500);

    // 0 会被抬到下限（解析层已拦 0，这里兜住直接调用的情况）
    cx.update(|_, app| panel.update(app, |p, cx| p.set_segment_rows(0, cx)));
    assert_eq!(
        cx.update(|_, app| panel.read(app).segment_rows()),
        crate::ui::RESULT_SEGMENT_ROWS_MIN
    );
}

/// 【R6 段大小】状态行真的摆出「段」输入框（面板注入 InputState 后才有该元素）。
#[gpui_kit::test]
fn status_row_shows_the_segment_size_input(cx: &mut gpui_kit::TestAppContext) {

    cx.update(gpui_kit::init);
    let (panel, cx) = panel_harness(cx);
    // 状态行只在「有 status / 有影响行数 / 失败 / 有耗时」时出现（`render.rs` 的
    // `show_status` 口径），测试壳默认不注入 status，这里补一份最小的
    cx.update(|_, app| {
        panel.update(app, |p, _cx| {
            p.status = Some(crate::grid::status::ResultStatus {
                total_rows: 2,
                ..Default::default()
            });
        })
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));

    assert!(
        cx.debug_bounds("editor-result-segment-size").is_some(),
        "状态行应摆出段大小输入框"
    );
    assert!(
        cx.debug_bounds("editor-result-status-row").is_some(),
        "状态行本体应在（段框是它的一段）"
    );
}

/// 【R6 段大小·持久化闭环】构造期注入持久化值：状态落位，且输入框显示该值
/// （只有占位 1000 时，用户在状态行上看不到「我上次设的是 500」）。
#[gpui_kit::test]
fn set_segment_rows_initial_puts_the_value_in_the_box(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let (panel, cx) = panel_harness(cx);
    cx.update(|window, app| {
        panel.update(app, |p, cx| p.set_segment_rows_initial(500, window, cx))
    });

    assert_eq!(cx.update(|_, app| panel.read(app).segment_rows()), 500);
    let text = cx.update(|_, app| {
        let input = panel.read(app).segment_input.clone();
        input.read(app).value().to_string()
    });
    assert_eq!(text, "500", "注入后输入框应显示持久化值而非留空");
}

/// 【R7 计算页右键菜单】「复制全部」文本按 DBeaver `CopyAllAction` 的非分组口径：
/// 逐行 `函数=值`、`\n` 分隔；空集合 = 空串（调用点凭空串跳过写剪贴板）。
#[test]
fn stats_copy_all_text_joins_function_equals_value() {
    use crate::panel::tabs::stats_copy_all_text;
    use crate::ports::StatsItem;

    let entries = vec![
        (StatsItem::Count, "12".to_string()),
        (StatsItem::Min, "3.5".to_string()),
    ];
    assert_eq!(stats_copy_all_text(&entries), "非空=12\n最小值=3.5");

    let single = vec![(StatsItem::Count, "0".to_string())];
    assert_eq!(stats_copy_all_text(&single), "非空=0");

    assert_eq!(stats_copy_all_text(&[]), "");
}

/// 【页签提示】六页各有一条非空、互不重复的提示文案（对齐 DBeaver 面板描述：
/// 页签只有图标 + 短名，靠 tooltip 说清这页做什么）。
#[test]
fn right_tab_descriptions_are_distinct_and_non_empty() {
    use crate::panel::events::{right_tab_description, right_tabs};

    let mut seen: Vec<&'static str> = Vec::new();
    for tab in right_tabs() {
        let description = right_tab_description(tab);
        assert!(!description.is_empty(), "{tab:?} 缺少页签提示");
        assert!(
            !seen.contains(&description),
            "{tab:?} 的提示与其它页重复：{description}"
        );
        seen.push(description);
    }
    assert_eq!(seen.len(), 6, "六页各一条提示");
}

/// 【值页动作可用性】「保存值到文件」的判据（对齐 DBeaver `!content.isNull()` 才贡献
/// `saveToFile`）：NULL / 缺列 / 行号槽都不给入口，真实值才给。
#[test]
fn value_save_is_gated_by_null_and_row_number_slot() {
    use crate::panel::tabs::value_can_save_to_file;

    assert!(value_can_save_to_file(1, Some("1234.5")));
    assert!(value_can_save_to_file(2, Some("")));
    assert!(!value_can_save_to_file(1, Some("NULL")));
    assert!(!value_can_save_to_file(1, None));
    // 行号槽（col == 0）显示的是视图行号，不是数据格
    assert!(!value_can_save_to_file(0, Some("3")));
}

/// 【值页工具条】「在外部程序中打开」入口常驻（对齐 `ContentValueManager` 的
/// 「Open in external editor」，与保存至文件同一处贡献）。零内容时不生效由
/// `value_can_save_to_file` 同口径置灰覆盖。
#[gpui_kit::test]
fn value_toolbar_offers_open_in_external_app(cx: &mut gpui_kit::TestAppContext) {

    cx.update(gpui_kit::init);
    let (panel, cx) = panel_harness(cx);
    // 出厂默认页是列浏览器；本用例要验值页工具条（空态是另一条渲染分支，没有这几颗动作）
    cx.update(|_, app| {
        panel.update(app, |p, cx| {
            p.selection_pos = Some((0, 1));
            p.set_right_tab(RightTab::Value, cx);
        })
    });

    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(
        cx.debug_bounds("editor-result-value-open-external").is_some(),
        "值页工具条应有「在外部程序中打开」"
    );
    assert!(
        cx.debug_bounds("editor-result-value-save").is_some(),
        "同一行里保存至文件应仍在"
    );
}

/// 【值页动作可用性】选中 NULL 格时 `selected_value_payload` 给 `None`
/// （宿主据此提示「没有内容」，不会写一个内容为 `NULL` 的文件）。
#[gpui_kit::test]
fn null_cell_has_no_value_payload(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let (panel, cx) = panel_harness(cx);
    // 第 2 行 amount 列 = NULL；第 1 行同一列 = 1234.5
    cx.update(|_window, app| {
        panel.update(app, |p, cx| {
            p.selection_pos = Some((1, 2));
            cx.notify();
        })
    });
    let payload = panel.read_with(cx, |p, app| p.selected_value_payload(app));
    assert!(payload.is_none(), "NULL 格不应给出落盘载荷");

    cx.update(|_window, app| {
        panel.update(app, |p, cx| {
            p.selection_pos = Some((0, 2));
            p.set_right_tab(crate::ports::RightTab::Value, cx);
        })
    });
    let payload = panel.read_with(cx, |p, app| p.selected_value_payload(app));
    assert!(payload.is_some(), "有值的格应给出落盘载荷");
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

    // tooltip 口径：目标状态描述 + 命令描述（DBeaver `describeState + " (" + 命令 + ")"`，
    // `ResultSetFilterPanel.java:456-475`）；描述里的空白折叠、超 50 字符截断
    let hint = cx.update(|_, app| panel.read(app).history_hint(-1));
    assert_eq!(
        hint, "select 1 (后退历史)",
        "tooltip 应是目标状态 SQL + 命令描述"
    );
    let hint = cx.update(|_, app| panel.read(app).history_hint(1));
    assert_eq!(
        hint, "已经在最新的状态",
        "已在末位时给说明（DBeaver 禁用态不刷新 tooltip，本仓保留一句）"
    );
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

    // ① 条目顺序：六页 → 分隔 → 最大化 → 分隔 → 面板总开关 → 布局方向
    //    （对齐 DBeaver `fillPanelsMenu:1559-1588` 的两条分隔线段位）
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
    assert_eq!(items.len(), 11, "六页 + 分隔 + 最大化 + 分隔 + 面板开关 + 布局方向");
    assert_eq!(items[6], super::events::PanelViewItem::Separator);
    assert_eq!(items[7], super::events::PanelViewItem::MaximizeToggle);
    assert_eq!(items[8], super::events::PanelViewItem::Separator);
    assert_eq!(items[9], super::events::PanelViewItem::PanelsToggle);
    assert_eq!(items[10], super::events::PanelViewItem::LayoutToggle);
    // 没有激活面板（`CMD_ACTIVATE_PANELS`）：本仓没有端口，不摆点了没反应的条目
    assert!(
        items
            .iter()
            .all(|item| matches!(item, super::events::PanelViewItem::Page(_))
                || matches!(
                    item,
                    super::events::PanelViewItem::Separator
                        | super::events::PanelViewItem::PanelsToggle
                        | super::events::PanelViewItem::LayoutToggle
                        | super::events::PanelViewItem::MaximizeToggle
                )),
        "菜单里只应有五类条目"
    );

    // ② 文案：页用页面名，开关报「点了会做什么」
    let label = super::events::panel_view_item_label;
    let page = super::events::PanelViewItem::Page(RightTab::Stats);
    assert_eq!(label(page, true, false, false), "计算");
    let panels = super::events::PanelViewItem::PanelsToggle;
    assert_eq!(
        label(panels, true, false, false),
        "收起结果面板",
        "面板可见时报「收起」"
    );
    assert_eq!(
        label(panels, false, false, false),
        "展开结果面板",
        "面板收起时报「展开」"
    );
    let layout = super::events::PanelViewItem::LayoutToggle;
    assert_eq!(label(layout, true, false, false), "切换为竖排布局（上下分栏）");
    assert_eq!(label(layout, true, true, false), "切换为横排布局（左右分栏）");
    assert_eq!(
        label(super::events::PanelViewItem::Separator, true, false, false),
        ""
    );
    // 最大化报动作：未最大化 → 去最大化；已最大化 → 还原
    let maximize = super::events::PanelViewItem::MaximizeToggle;
    assert_eq!(
        label(maximize, true, false, false),
        "面板最大化（收起结果网格）"
    );
    assert_eq!(label(maximize, true, false, true), "还原结果网格");

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

    // 列清单初始：两列都可见、都没冻结、无条件、无排序
    let rows = panel.read_with(cx, |p, app| p.column_constraint_rows(app));
    assert_eq!(
        rows,
        vec![
            crate::ports::ColumnConstraint {
                name: "id".to_string(),
                visible: true,
                frozen: false,
                criteria: None,
                sort: None,
            },
            crate::ports::ColumnConstraint {
                name: "amount".to_string(),
                visible: true,
                frozen: false,
                criteria: None,
                sort: None,
            }
        ],
        "初始列清单应是「两列都可见、未冻结、无条件、无排序」"
    );

    // 隐藏第 1 列（走与列浏览器同一通道的批量接口），投影应立刻变 false
    cx.update(|_window, app| {
        panel.update(app, |p, cx| p.set_columns_hidden(vec![0], true, cx))
    });
    let rows = panel.read_with(cx, |p, app| p.column_constraint_rows(app));
    assert!(!rows[0].visible, "隐藏后 id 列应显示为不可见：{rows:?}");
    assert!(rows[1].visible, "另一列不该被牵连：{rows:?}");

    // 冻结第 2 列（直接改 delegate，与列头菜单 ToggleFreeze 同一条路）
    cx.update(|_window, app| {
        let grid = panel.read(app).grid.clone();
        grid.update(app, |state, app| {
            state.delegate_mut().toggle_freeze(1);
            state.refresh(app);
        });
    });
    let rows = panel.read_with(cx, |p, app| p.column_constraint_rows(app));
    assert!(rows[1].frozen, "冻结后 amount 列应显示为已冻结：{rows:?}");
    assert!(!rows[0].frozen, "id 列不该被冻结：{rows:?}");

    // 工具条「重置列布局」：显示全部 + 取消全部冻结
    cx.update(|_window, app| {
        panel.update(app, |p, cx| p.reset_column_layout(cx))
    });
    let rows = panel.read_with(cx, |p, app| p.column_constraint_rows(app));
    assert!(
        rows.iter().all(|row| row.visible && !row.frozen),
        "重置后应全部可见且全部未冻结：{rows:?}"
    );

    // 条件 / 排序两列是**只读投影**（不是常量「—」）：把 delegate 的列级筛选与本地排序
    // 写进去，投影要跟着变（列级筛选从列头菜单来、本地排序从列头点击来）
    cx.update(|_window, app| {
        let grid = panel.read(app).grid.clone();
        grid.update(app, |state, app| {
            state.delegate_mut().set_column_filter(Some((1, "> 100".to_string())));
            state.delegate_mut().apply_sort(Some((1, true)));
            state.refresh(app);
        });
    });
    let rows = panel.read_with(cx, |p, app| p.column_constraint_rows(app));
    assert_eq!(
        rows[1].criteria.as_deref(),
        Some("> 100"),
        "amount 列的条件应投影出列级筛选文本：{rows:?}"
    );
    assert_eq!(rows[1].sort, Some(true), "amount 列应投影出降序：{rows:?}");
    assert_eq!(rows[0].criteria, None, "id 列不该有条件：{rows:?}");
    assert_eq!(rows[0].sort, None, "id 列不该有排序：{rows:?}");

    // 工具条「全部不显示」：逐列 hide 有「至少留 1 列可见」底线，故只留一列
    cx.update(|_window, app| {
        panel.update(app, |p, cx| p.hide_all_columns(cx))
    });
    let rows = panel.read_with(cx, |p, app| p.column_constraint_rows(app));
    assert_eq!(
        rows.iter().filter(|row| row.visible).count(),
        1,
        "全部不显示后应只剩 1 列可见（本仓底线）：{rows:?}"
    );

    // 工具条「全部显示」：回满
    cx.update(|_window, app| {
        panel.update(app, |p, cx| p.show_all_columns(cx))
    });
    let rows = panel.read_with(cx, |p, app| p.column_constraint_rows(app));
    assert!(
        rows.iter().all(|row| row.visible),
        "全部显示后每列都可见：{rows:?}"
    );

    // 工具条移动四钮 / 排序：作用在**当前选中列**（harness 默认选中数据列 1 = amount）
    cx.update(|_window, app| {
        panel.update(app, |p, cx| {
            p.move_selected_column(crate::ports::ColumnMove::Top, cx)
        })
    });
    let order = panel.read_with(cx, |p, app| p.grid.read(app).delegate().display_order());
    assert_eq!(
        order,
        vec!["amount".to_string(), "id".to_string()],
        "移顶后显示序应把 amount 挪到最前"
    );
    assert_eq!(
        panel.read_with(cx, |p, _| p.column_order()),
        order,
        "列序快照要跟着移动走（宿主据此落库）"
    );

    // 已在首位再「移顶」= 不动（与 DBeaver 按钮置灰同口径，但函数本身也要幂等）
    cx.update(|_window, app| {
        panel.update(app, |p, cx| {
            p.move_selected_column(crate::ports::ColumnMove::Top, cx)
        })
    });
    let order = panel.read_with(cx, |p, app| p.grid.read(app).delegate().display_order());
    assert_eq!(order, vec!["amount".to_string(), "id".to_string()], "重复移顶不该乱序");

    // 移底：回到数据序
    cx.update(|_window, app| {
        panel.update(app, |p, cx| {
            p.move_selected_column(crate::ports::ColumnMove::Bottom, cx)
        })
    });
    let order = panel.read_with(cx, |p, app| p.grid.read(app).delegate().display_order());
    assert_eq!(order, vec!["id".to_string(), "amount".to_string()], "移底后回数据序");

    // 按列名排序：amount / id
    cx.update(|_window, app| {
        panel.update(app, |p, cx| p.sort_columns_by_name(cx))
    });
    let order = panel.read_with(cx, |p, app| p.grid.read(app).delegate().display_order());
    assert_eq!(
        order,
        vec!["amount".to_string(), "id".to_string()],
        "按列名 A→Z 重排"
    );

    // 重置：列序回数据序
    cx.update(|_window, app| {
        panel.update(app, |p, cx| p.reset_column_layout(cx))
    });
    let order = panel.read_with(cx, |p, app| p.grid.read(app).delegate().display_order());
    assert_eq!(order, vec!["id".to_string(), "amount".to_string()], "重置后回数据序");
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

/// 【引用页】清单状态：回填后默认选第一条，长度不变时保住原选中，清单变空就清掉。
///
/// 口径照 DBeaver `ReferencesResultsContainer.fillKeysCombo`：`activeReferenceKey`
/// 保不住时回落到 `referenceKeys.getFirst()`（`:341-343`）。
#[gpui_kit::test]
fn references_state_selects_first_key_and_drops_a_stale_selection(
    cx: &mut gpui_kit::TestAppContext,
) {
    use crate::references::{ReferenceKey, ReferencesState};

    cx.update(gpui_kit::init);
    let (panel, cx) = panel_harness(cx);

    let key = |table: &str| ReferenceKey {
        table: table.to_string(),
        constraint: "fk".to_string(),
        columns: vec!["id".to_string()],
        target_columns: vec!["id".to_string()],
    };

    // 读取中：没有可选键
    cx.update(|_window, app| {
        panel.update(app, |p, cx| p.set_references(ReferencesState::Loading, cx))
    });
    assert_eq!(
        panel.read_with(cx, |p, _| p.active_reference),
        None,
        "读取中没有可选项"
    );

    // 回填两条 → 默认第一条
    cx.update(|_window, app| {
        panel.update(app, |p, cx| {
            p.set_references(
                ReferencesState::Loaded(vec![key("customers"), key("regions")]),
                cx,
            )
        })
    });
    assert_eq!(
        panel.read_with(cx, |p, _| p.active_reference),
        Some(0),
        "回填后默认选第一条"
    );

    // 用户改选第二条后，清单长度不变再回填 → 保住原选中
    cx.update(|_window, app| panel.update(app, |p, cx| p.set_active_reference(Some(1), cx)));
    cx.update(|_window, app| {
        panel.update(app, |p, cx| {
            p.set_references(
                ReferencesState::Loaded(vec![key("customers"), key("regions")]),
                cx,
            )
        })
    });
    assert_eq!(
        panel.read_with(cx, |p, _| p.active_reference),
        Some(1),
        "长度不变时应保住用户选的那条"
    );

    // 清单变空 → 选中清掉
    cx.update(|_window, app| {
        panel.update(app, |p, cx| {
            p.set_references(ReferencesState::Loaded(Vec::new()), cx)
        })
    });
    assert_eq!(
        panel.read_with(cx, |p, _| p.active_reference),
        None,
        "没有键时不该留一个悬空下标"
    );
}

/// 【引用页】来源表身份：只有「所有已注解列都属于同一张表」才认；隐藏列不算可见列。
#[gpui_kit::test]
fn reference_source_needs_one_source_table_and_drops_hidden_columns(
    cx: &mut gpui_kit::TestAppContext,
) {
    use workbench_shell::model::ColumnAnnotation;

    cx.update(gpui_kit::init);
    let (panel, cx) = panel_harness(cx);

    let annotation = |table: &str, column: &str| ColumnAnnotation {
        source_table: Some(table.to_string()),
        source_column: Some(column.to_string()),
        comment: None,
        data_type: None,
        nullable: None,
        default_value: None,
    };

    // 两列都来自 public.orders；隐藏 amount 之后可见列只剩 id
    cx.update(|_window, app| {
        let grid = panel.read(app).grid.clone();
        grid.update(app, |state, _cx| {
            state.delegate_mut().set_column_annotations(vec![
                Some(annotation("public.orders", "id")),
                Some(annotation("public.orders", "amount")),
            ]);
        });
        panel.update(app, |p, _cx| {
            p.column_hidden = vec!["amount".to_string()];
        });
    });
    let source = panel.read_with(cx, |p, app| p.reference_source(app));
    assert_eq!(
        source,
        Some((
            "public".to_string(),
            "orders".to_string(),
            vec!["id".to_string()]
        )),
        "来源表拆成 schema + 表名，隐藏列不进可见列"
    );

    // 两列来自不同表 → 没有单一来源表（多表 join 的结果不看外键清单）
    cx.update(|_window, app| {
        let grid = panel.read(app).grid.clone();
        grid.update(app, |state, _cx| {
            state.delegate_mut().set_column_annotations(vec![
                Some(annotation("public.orders", "id")),
                Some(annotation("public.items", "amount")),
            ]);
        });
    });
    assert_eq!(
        panel.read_with(cx, |p, app| p.reference_source(app)),
        None,
        "多来源表不该认成某一张表"
    );
}

/// 【引用页】切页签即入队取数；有键时渲染只读键下拉 + 「打开目标对象」。
#[gpui_kit::test]
fn reference_tab_requests_metadata_and_renders_the_key_list(cx: &mut gpui_kit::TestAppContext) {
    use crate::references::{ReferenceKey, ReferencesState};
    use workbench_shell::model::ColumnAnnotation;

    cx.update(gpui_kit::init);
    let (panel, cx) = panel_harness(cx);

    cx.update(|_window, app| {
        let grid = panel.read(app).grid.clone();
        grid.update(app, |state, _cx| {
            state.delegate_mut().set_column_annotations(vec![
                Some(ColumnAnnotation {
                    source_table: Some("public.orders".to_string()),
                    source_column: Some("id".to_string()),
                    comment: None,
                    data_type: None,
                    nullable: None,
                    default_value: None,
                }),
                Some(ColumnAnnotation {
                    source_table: Some("public.orders".to_string()),
                    source_column: Some("amount".to_string()),
                    comment: None,
                    data_type: None,
                    nullable: None,
                    default_value: None,
                }),
            ]);
        });
        panel.update(app, |p, cx| p.begin_references_load(cx));
    });

    let drained = cx.update(|_window, app| panel.read(app).queue.drain());
    assert_eq!(drained.len(), 1, "取数只该入队一条请求：{drained:?}");
    match &drained[0] {
        crate::ports::ResultRequest::LoadReferences {
            schema,
            table,
            visible_columns,
        } => {
            assert_eq!(schema, "public");
            assert_eq!(table, "orders");
            assert_eq!(
                visible_columns,
                &vec!["id".to_string(), "amount".to_string()]
            );
        }
        other => panic!("应入队 LoadReferences，实际 {other:?}"),
    }

    // 回填一条键并把右栏切到引用页：下拉与「打开目标对象」都要在元素树里
    cx.update(|_window, app| {
        panel.update(app, |p, cx| {
            p.set_references(
                ReferencesState::Loaded(vec![ReferenceKey {
                    table: "customers".to_string(),
                    constraint: "fk_customer".to_string(),
                    columns: vec!["id".to_string()],
                    target_columns: vec!["id".to_string()],
                }]),
                cx,
            );
            p.set_right_tab(RightTab::Reference, cx);
        });
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(
        cx.debug_bounds("editor-result-reference-keys").is_some(),
        "有键时该渲染只读键下拉"
    );
    assert!(
        cx.debug_bounds("editor-result-reference-open").is_some(),
        "有键时该渲染「打开目标对象」"
    );
}

/// 【引用页】「查看引用行」：选中键 + 当前行 → 生成目标表查询，并按 `B′` 交给宿主另开结果集。
///
/// 口子对齐 DBeaver `refreshKeyValues`（`:382-432`）：查什么由**当前行的外键值**决定，
/// 不是整表；本仓把这条查询交给编辑器新开一份结果集，不在面板里嵌第二份视图。
#[gpui_kit::test]
fn reference_rows_use_the_selected_row_values(cx: &mut gpui_kit::TestAppContext) {
    use crate::references::{ReferenceKey, ReferencesState};
    use workbench_shell::model::ColumnAnnotation;

    cx.update(gpui_kit::init);
    let (panel, cx) = panel_harness(cx);

    // 结果两列来自 public.orders：id(integer) / amount(numeric)
    cx.update(|_window, app| {
        let grid = panel.read(app).grid.clone();
        grid.update(app, |state, _cx| {
            state.delegate_mut().set_column_annotations(vec![
                Some(ColumnAnnotation {
                    source_table: Some("public.orders".to_string()),
                    source_column: Some("id".to_string()),
                    comment: None,
                    data_type: Some("integer".to_string()),
                    nullable: None,
                    default_value: None,
                }),
                Some(ColumnAnnotation {
                    source_table: Some("public.orders".to_string()),
                    source_column: Some("amount".to_string()),
                    comment: None,
                    data_type: Some("numeric".to_string()),
                    nullable: None,
                    default_value: None,
                }),
            ]);
        });
    });
    cx.update(|_window, app| {
        panel.update(app, |p, cx| {
            p.set_references(
                ReferencesState::Loaded(vec![ReferenceKey {
                    table: "customers".to_string(),
                    constraint: "fk_customer".to_string(),
                    columns: vec!["id".to_string()],
                    target_columns: vec!["id".to_string()],
                }]),
                cx,
            );
        });
    });

    // 没选中行时生成不了（按钮禁用态由它决定）
    assert_eq!(
        panel.read_with(cx, |p, app| p.reference_rows_sql(app)),
        None,
        "没有当前行就没有值可查"
    );

    // 选中第一行（id = 1）后能生成：数值列裸拼，字符串列加引号
    cx.update(|_window, app| {
        let grid = panel.read(app).grid.clone();
        grid.update(app, |state, _cx| {
            state.delegate_mut().set_selected_row(Some(0));
        });
    });
    let sql = panel
        .read_with(cx, |p, app| p.reference_rows_sql(app))
        .expect("选中行后应能生成引用行查询");
    assert_eq!(sql, "SELECT * FROM customers WHERE id = 1");

    // 交给宿主：请求里带的是同一条 SQL（宿主用 NewSet 落位）
    cx.update(|_window, app| panel.update(app, |p, cx| p.open_reference_rows(cx)));
    let drained = cx.update(|_window, app| panel.read(app).queue.drain());
    assert_eq!(drained.len(), 1, "只该入队一条请求：{drained:?}");
    match &drained[0] {
        crate::ports::ResultRequest::OpenReferenceRows { sql: requested } => {
            assert_eq!(requested, &sql);
        }
        other => panic!("应入队 OpenReferenceRows，实际 {other:?}"),
    }

    // 页面上也要有这张票（render 路径与上面同一个函数算可用性）
    cx.update(|_window, app| panel.update(app, |p, cx| p.set_right_tab(RightTab::Reference, cx)));
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(
        cx.debug_bounds("editor-result-reference-rows").is_some(),
        "有键且选中行时该渲染「查看引用行」"
    );
}



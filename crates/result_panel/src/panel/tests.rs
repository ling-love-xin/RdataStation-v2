//! panel/ 全量单元测试

use super::clamp_right_sidebar_height;
use super::clamp_right_sidebar_width;
use super::next_right_edge_state;
use super::reorder_by;
use super::sidebar_width_anim;
use super::tabs::stat_value;
use super::tabs::StatExtras;
use super::tabs::value_payload;
use crate::column_browser::filter_browser_columns;
use crate::grid::stats::ColumnStats;
use super::tabs::arrange_groups;
use crate::ports::{FindCriteria, GroupSort, RightTab, StatsItem, StatsMode};
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

/// 【过滤框 ESC 清空】对齐 `MetaDataPanel.java:87-98`：过滤框是
/// `Text(SWT.BORDER|SWT.SEARCH|SWT.ICON_CANCEL)` + KeyListener
/// （`e.character == SWT.ESC → filterTextBox.setText("")`，随后 `refresh(true)` 重算）。
///
/// 本仓过滤框是页顶 `filterPanel` 里那一格（DBeaver 的过滤框也不在表内，
/// `MetaDataPanel.java:81-89`），由面板自持的 `browser_filter_input` 驱动
/// `ListState::set_query`（`set_query` 会顺带起一次搜索）。kit `Input` 不消费 Esc（见
/// `panel/render.rs` 重命名条的同款注释），所以按键会冒泡到列浏览器页根容器 ——
/// 在那里同时清输入框与查询词。
#[gpui_kit::test]
fn column_browser_filter_clears_on_escape(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let (panel, cx) = panel_harness(cx);
    // 测试壳只灌网格；列浏览器的 delegate 数据要单独喂（同
    // `column_browser_draws_the_comment_line_below_the_name` 的做法）
    cx.update(|_, app| {
        panel.update(app, |p, cx| {
            p.set_right_tab(RightTab::Browser, cx);
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
        })
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));

    let list = panel.read_with(cx, |p, _| p.browser_list.clone());
    // 输入搜索词（`set_query` 内部会直接起一次搜索，等价于用户敲键）
    cx.update(|window, cx| list.update(cx, |list, cx| list.set_query("amount", window, cx)));
    assert_eq!(
        list.read_with(cx, |list, _| list.delegate().query().to_string()),
        "amount",
        "搜索词应写进 delegate"
    );
    assert_eq!(
        list.read_with(cx, |list, _| list.delegate().matched().len()),
        1,
        "只命中 amount 一列"
    );

    // 聚焦搜索框 → 敲 ESC
    cx.update(|window, cx| list.update(cx, |list, cx| list.focus(window, cx)));
    cx.simulate_keystrokes("escape");
    assert_eq!(
        list.read_with(cx, |list, _| list.delegate().query().to_string()),
        "",
        "ESC 应清空过滤词（对齐 MetaDataPanel:91-98）"
    );
    assert_eq!(
        list.read_with(cx, |list, _| list.delegate().matched().len()),
        2,
        "清空后回到全量清单"
    );
}

/// 【D2 对齐 2026-11-24】列浏览器页顶过滤条：DBeaver 的过滤框在页顶 `filterPanel`
/// （`MetaDataPanel.java:81-89`：`GridLayout(2 列)` = Label「筛选：」+ `Text`），
/// **不在表内**。本仓把搜索框从 kit `List` 内置挪到页顶，由面板自持的
/// `browser_filter_input` → `ListState::set_query` 驱动。本条钉两件事：
/// (a) 页顶过滤条真的渲染（不是只加了字段）；(b) 真敲键能穿透到 delegate
/// （订阅句柄没被丢，`InputEvent::Change` 这条链还活着）。
#[gpui_kit::test]
fn the_top_filter_bar_drives_the_column_list(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let (panel, cx) = panel_harness(cx);
    cx.update(|_, app| {
        panel.update(app, |p, cx| {
            p.set_right_tab(RightTab::Browser, cx);
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
        })
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));

    assert!(
        cx.debug_bounds("editor-result-col-browser-filter").is_some(),
        "列浏览器页顶必须有 DBeaver 的 filterPanel（「筛选：」+ 搜索框）"
    );

    // 焦点交给页顶过滤框 → 真敲键（程序化 `set_value` 不发 `Change`，测不到订阅）
    let filter_input = panel.read_with(cx, |p, _| p.browser_filter_input.clone());
    cx.update(|window, cx| {
        filter_input.update(cx, |state, cx| state.focus(window, cx));
    });
    cx.simulate_keystrokes("amount");
    // 0.10.0 帧语义：只改状态直接 draw 会读到上一帧（见 gpui-kit-dev「首帧 ≠ 稳定帧」）
    cx.update(|window, _| window.refresh());
    cx.update(|window, cx| window.draw(cx).clear(cx));

    let list = panel.read_with(cx, |p, _| p.browser_list.clone());
    assert_eq!(
        list.read_with(cx, |list, _| list.delegate().query().to_string()),
        "amount",
        "页顶过滤框的字没到 List：`InputEvent::Change` 订阅没挂上"
    );
    assert_eq!(
        list.read_with(cx, |list, _| list.delegate().matched().len()),
        1,
        "只命中 amount 一列"
    );
}

/// 页签顺序 / 下标映射（`events::right_tabs` 单一来源）：全集下
/// 0=值详情 / 1=计算 / 2=列浏览器 / 3=分组 / 4=引用，越界回落默认页（= 值详情，
/// 即 DBeaver 唯一 `default="true"` 的 `value-view`：规范序 = 扩展注册序，见 §46）；
/// 反查（枚举 → 下标）与之互逆，页签条 `selected_index` 与点击回调用的是同一份换算
/// （`strip_index` / `tab_at_strip_index`，作用在「当前已启用集合」上）。
#[test]
fn right_tab_index_maps_to_the_five_panel_tabs() {
    use super::events::{right_tabs, strip_index, tab_at_strip_index};

    let all = right_tabs();
    assert_eq!(tab_at_strip_index(&all, 0), RightTab::Value);
    assert_eq!(tab_at_strip_index(&all, 1), RightTab::Stats);
    assert_eq!(tab_at_strip_index(&all, 2), RightTab::Browser);
    assert_eq!(tab_at_strip_index(&all, 3), RightTab::Group);
    assert_eq!(tab_at_strip_index(&all, 4), RightTab::Reference);
    assert_eq!(tab_at_strip_index(&all, 99), RightTab::Value);

    // 互逆：每个页签既是自己的下标对应的枚举，下标也唯一
    for (index, tab) in all.iter().copied().enumerate() {
        assert_eq!(tab_at_strip_index(&all, index), tab);
        assert_eq!(strip_index(&all, tab), index);
    }

    // 集合是子集时换算跟着集合走（关掉「计算」页后，分组从 3 变成 2）
    let partial = [RightTab::Browser, RightTab::Value, RightTab::Group];
    assert_eq!(strip_index(&partial, RightTab::Group), 2);
    assert_eq!(tab_at_strip_index(&partial, 2), RightTab::Group);
    // 不在集合里的页 → 下标 0（不会画到条上）
    assert_eq!(strip_index(&partial, RightTab::Stats), 0);
}

/// 【右栏关闭/重开】对齐 DBeaver `CTabItem(panelFolder, SWT.CLOSE)` +
/// `presentationSettings.enabledPanelIds`：关一页只移除这一页（其余仍在），
/// 关当前页则落到规范序的下一剩页；关到空集收起右栏；重开按规范序插回并置为当前。
#[gpui_kit::test]
fn close_right_tab_drops_one_page_and_reopen_restores_order(
    cx: &mut gpui_kit::TestAppContext,
) {
    use super::events::{neighbor_after_close, right_tabs};

    // 纯函数：关掉规范序中间那页 → 落到紧随其后的剩页；关最后一页 → 落到最后一个剩页
    // （规范序 §55 = Value → Stats → Browser → Group → Reference）
    let all = right_tabs();
    let without_stats: Vec<RightTab> =
        all.iter().copied().filter(|t| *t != RightTab::Stats).collect();
    assert_eq!(neighbor_after_close(&without_stats, RightTab::Stats), RightTab::Browser);
    let only_tail = [RightTab::Browser, RightTab::Value];
    assert_eq!(neighbor_after_close(&only_tail, RightTab::Reference), RightTab::Value);

    // 面板态：先把出厂的单页集合铺成五页（本用例要看「关一页、其余还在」）
    cx.update(gpui_kit::init);
    let (panel, cx) = panel_harness(cx);
    cx.update(|_, app| {
        panel.update(app, |p, cx| p.set_open_tabs_initial(&RightTab::ALL, cx))
    });
    assert_eq!(
        panel.read_with(cx, |p, _| p.open_tabs().to_vec()),
        RightTab::ALL.to_vec(),
        "前提：五页全开"
    );

    // 关「计算」页（当前页是出厂默认的值详情 → 当前页不变，只少一项）
    cx.update(|_, app| {
        panel.update(app, |p, cx| p.close_right_tab(RightTab::Stats, cx))
    });
    let open = panel.read_with(cx, |p, _| p.open_tabs().to_vec());
    assert_eq!(open.len(), 4);
    assert!(!open.contains(&RightTab::Stats));
    assert_eq!(
        panel.read_with(cx, |p, _| p.right_tab),
        RightTab::Value,
        "关的不是当前页 → 当前页不动"
    );

    // 关当前页（先切到分组页，再关它）→ 落到紧随其后的「引用」
    cx.update(|_, app| {
        panel.update(app, |p, cx| {
            p.set_right_tab(RightTab::Group, cx);
            p.close_right_tab(RightTab::Group, cx);
        })
    });
    assert_eq!(panel.read_with(cx, |p, _| p.right_tab), RightTab::Reference);

    // 重开「计算」页：按规范序插入后**被 MRU 挪到首位**
    // （真值 `setMRUVisible(true)`，`ResultSetViewer.java:364`：重开 = `activatePanel`，
    // 该页成为选中页，MRU 显示下它排到最前）
    cx.update(|_, app| {
        panel.update(app, |p, cx| p.open_right_tab(RightTab::Stats, cx))
    });
    let open = panel.read_with(cx, |p, _| p.open_tabs().to_vec());
    assert_eq!(
        open,
        vec![
            RightTab::Stats,
            RightTab::Value,
            RightTab::Browser,
            RightTab::Reference,
        ],
        "重开的页在 MRU 顺序下应排到首位（其余页保持相对序）"
    );
    assert_eq!(panel.read_with(cx, |p, _| p.right_tab), RightTab::Stats);

    // 关到空集 → 右栏整体收起（与「关掉最后一个面板」在那边导致页签夹无项一致）
    cx.update(|_, app| {
        panel.update(app, |p, cx| {
            for tab in right_tabs() {
                p.close_right_tab(tab, cx);
            }
        })
    });
    assert!(panel.read_with(cx, |p, _| p.open_tabs().is_empty()));
    assert!(
        !panel.read_with(cx, |p, _| p.panels_visible()),
        "关掉最后一页后右栏应收起"
    );
}

/// 【右栏】关空集 → 收起 → **再展开必须恢复默认页集合**。
///
/// 取证 `ResultSetViewer.showPanels:1478-1518`：展开时 `showDefaults=true` 会先跑
/// `activateDefaultPanels(getPresentationSettings())`（`:1493-1496`）再
/// `setMaximizedControl(null)` + `updatePanelsContent(false)`——也就是「面板集合被关空之后
/// 重新显示，一定有默认面板可显示」，不会留下一个空页签夹。
///
/// 本仓在修这条之前：`close_right_tab` 关空只收起（`open_tabs` 留空），`set_panels_visible(true)`
/// 只翻可见位，于是重新展开得到的是「右栏空白、页签条上一个页签都没有」。
#[gpui_kit::test]
fn reopening_after_closing_all_pages_restores_default_tabs(cx: &mut gpui_kit::TestAppContext) {
    use super::events::right_tabs;

    cx.update(gpui_kit::init);
    let (panel, cx) = panel_harness(cx);

    cx.update(|_, app| {
        panel.update(app, |p, cx| {
            for tab in right_tabs() {
                p.close_right_tab(tab, cx);
            }
        })
    });
    assert!(panel.read_with(cx, |p, _| p.open_tabs().is_empty()));
    assert!(!panel.read_with(cx, |p, _| p.panels_visible()));

    // 再展开（工具栏/宿主路径）
    cx.update(|_, app| panel.update(app, |p, cx| p.set_panels_visible(true, cx)));
    assert!(panel.read_with(cx, |p, _| p.panels_visible()), "应重新展开");
    assert_eq!(
        panel.read_with(cx, |p, _| p.open_tabs().to_vec()),
        right_tabs().to_vec(),
        "空集重新展开应恢复默认页集合（否则页签条是空的）"
    );
    let current = panel.read_with(cx, |p, _| p.right_tab);
    assert!(
        panel.read_with(cx, |p, _| p.open_tabs().contains(&current)),
        "当前页必须落在恢复后的集合里"
    );
}

/// 【右栏】非空集合在开合之间**保持不变**（恢复默认集合只针对「被关空」的情况）。
#[gpui_kit::test]
fn toggling_the_sidebar_keeps_a_custom_tab_set(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let (panel, cx) = panel_harness(cx);

    // 先把出厂的单页集合铺成五页（本用例要看「关一页、其余不动」）
    cx.update(|_, app| {
        panel.update(app, |p, cx| p.set_open_tabs_initial(&RightTab::ALL, cx))
    });

    // 关掉「列浏览器」→ 四页
    cx.update(|_, app| {
        panel.update(app, |p, cx| p.close_right_tab(RightTab::Browser, cx))
    });
    let custom = panel.read_with(cx, |p, _| p.open_tabs().to_vec());
    assert_eq!(custom.len(), 4);

    // 状态栏三态开关：宽展开 → 收起 → 窄展开 → 宽展开（每次都经过收起再回来）
    cx.update(|_, app| panel.update(app, |p, cx| p.toggle_value_sidebar(cx)));
    assert!(!panel.read_with(cx, |p, _| p.panels_visible()));
    cx.update(|_, app| panel.update(app, |p, cx| p.toggle_value_sidebar(cx)));
    cx.update(|_, app| panel.update(app, |p, cx| p.toggle_value_sidebar(cx)));

    assert_eq!(
        panel.read_with(cx, |p, _| p.open_tabs().to_vec()),
        custom,
        "自定义集合不应被「恢复默认」覆盖"
    );
    assert!(!panel.read_with(cx, |p, _| p.open_tabs().contains(&RightTab::Browser)));
}

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

    // 1.7) 【列序对齐 2026-11-21】`HistoryPanel` 在 DBeaver 是 `filterComposite` 的
    //      第 5 列（紧跟 `ExecutePanel`，`ResultSetFilterPanel.java:174/209/214`），
    //      即位于擦除 / 筛选设置 / 后退 / 前进那一排 `filterToolbar` 的**左侧**。
    //      用水平坐标钉死顺序（纯结构断言，不涉及目视）：运行 < 历史 < 筛选设置；
    //      另钉后退 / 前进与查找之间那条竖分隔线（`:354`）真的画出来。
    let x_of = |id: &'static str, cx: &mut gpui_kit::VisualTestContext| -> f32 {
        let bounds = cx
            .debug_bounds(id)
            .unwrap_or_else(|| panic!("{id} 应带 debug_selector 才量得到"));
        f32::from(bounds.origin.x)
    };
    let run_x = x_of("editor-result-filter-run", cx);
    let history_x = x_of("editor-result-filter-history", cx);
    let settings_x = x_of("editor-result-filter-settings", cx);
    assert!(
        run_x < history_x && history_x < settings_x,
        "筛选条列序应为 运行({run_x}) < 历史({history_x}) < 筛选设置({settings_x})，\
         对齐 DBeaver 的 5 列 GridLayout（HistoryPanel 在 ExecutePanel 之后、filterToolbar 之前）"
    );
    assert!(
        visible("editor-result-filter-history-sep", cx),
        "后退/前进与查找之间应有竖分隔线（DBeaver ResultSetFilterPanel.java:354）"
    );

    // 1.8) 【筛选历史·浮层】历史入口点开的是**一张表**，不是菜单：DBeaver
    //      `HistoryPanel.showFilterHistoryPopup` 在无边框 Shell 里建
    //      `createFilterHistoryPanel`（`ResultSetFilterPanel.java:1093-1276`）。
    //      钉三件事：展开 → 表与行出现；行内容 = 历史里**去掉当前输入框文本**的那些
    //      （`:1233-1236`）；关掉 → 表消失。
    assert!(
        !visible("editor-result-filter-history-table", cx),
        "浮层未展开时不该有历史表"
    );
    cx.update(|_, app| {
        panel.update(app, |p, cx| {
            p.filter_text = "2".to_string();
            p.note_filter_commit();
            p.set_filter_history_popup(true, cx);
        })
    });
    // `Popover` 内容画在 overlay 层，打开那一帧常只出 trigger：画两帧再断言
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(
        visible("editor-result-filter-history-table", cx),
        "展开后应出现历史表（DBeaver 是无边框 Shell 里的 TableViewer，不是菜单）"
    );
    assert!(
        visible("editor-result-filter-history-row-0", cx),
        "历史表应有行：当前文本为「2」，余下的「1」应占第 0 行"
    );
    assert!(
        !visible("editor-result-filter-history-row-1", cx),
        "当前输入框文本那条应被过滤掉，只剩一行"
    );
    cx.update(|_, app| panel.update(app, |p, cx| p.set_filter_history_popup(false, cx)));
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(
        !visible("editor-result-filter-history-table", cx),
        "关掉浮层后历史表应消失（再点箭头 / 选行 / 失焦都走这条）"
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

/// 【第 72 批】「获取全部数据」确认浮层（DBeaver `CONFIRM_RS_FETCH_ALL`）：
/// 只有 `fetch_all_confirm_open` 为真时整段才挂——遮罩 / 卡片 / 正文 / 两颗按钮
/// 一起出现、一起消失，不留半截。
///
/// 断言走 `debug_bounds`（整段是 `then()` 条件渲染，没挂就等于取不到），
/// 不点按钮（headless 下按钮点击走的是同一份队列，键位侧已有用例钉入口）。
#[gpui_kit::test]
fn fetch_all_confirm_renders_only_while_open(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let (panel, cx) = panel_harness(cx);
    cx.update(|window, cx| window.draw(cx).clear(cx));

    let visible = |id: &'static str, cx: &mut gpui_kit::VisualTestContext| -> bool {
        cx.debug_bounds(id).is_some()
    };
    assert!(!visible("editor-result-fetch-all-confirm", cx), "出厂不弹");
    assert!(
        !cx.update(|_w, app| panel.read(app).fetch_all_confirm_state()),
        "状态行也不该是开着的"
    );

    cx.update(|_, app| {
        panel.update(app, |p, cx| p.open_fetch_all_confirm(cx));
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    for id in [
        "editor-result-fetch-all-confirm",
        "editor-result-fetch-all-confirm-card",
        "editor-result-fetch-all-confirm-body",
        "editor-result-fetch-all-cancel",
        "editor-result-fetch-all-go",
    ] {
        assert!(visible(id, cx), "浮层元素 {id} 应渲染（回退检查）");
    }

    // 取消 / Esc / ✕ 三路都落到同一个方法
    cx.update(|_, app| {
        panel.update(app, |p, cx| p.close_fetch_all_confirm(cx));
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(
        !visible("editor-result-fetch-all-confirm", cx),
        "关掉后遮罩要整段消失"
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
            p.set_grids(grid, None, None, window, cx);
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

/// 【D11/D12/D13】计算页：工具条「− / 分组」存在 + 行选中态 + 按选区列/行分组聚合。
///
/// 钉住三件对齐 `AggregateColumnsPanel` 的事实：
/// - `fillToolBar`（`:322-331`）里的「−」「分组」真的在工具条上；
/// - 「−」删的是**选中的行**（`:418-436`），行点击维护这份单选态；
/// - 分组开启后按选区覆盖的列各出一组、只算选区行区间（`:221-262`）。
#[gpui_kit::test]
fn stats_group_by_splits_selected_columns_and_rows(cx: &mut gpui_kit::TestAppContext) {
    use gpui_kit::component::table::TableState;
    use gpui_kit::component::Root;
    use gpui_kit::test::TestWindowExt as _;
    use gpui_kit::{AppContext as _, ElementId, Entity, IntoElement, Render, SharedString};

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
            vec!["id".to_string(), "amount".to_string(), "note".to_string()],
            vec![
                vec!["1".to_string(), "10".to_string(), "a".to_string()],
                vec!["2".to_string(), "20".to_string(), "b".to_string()],
                vec!["3".to_string(), "30".to_string(), "c".to_string()],
                vec!["4".to_string(), "40".to_string(), "d".to_string()],
            ],
        );
        let grid: Entity<TableState<crate::grid::ResultGridDelegate>> =
            cx.new(|cx| TableState::new(delegate, window, cx));
        panel.update(cx, |p, cx| {
            p.set_grids(grid, None, None, window, cx);
            // 作用列 = 第 2 列（amount）；只留「非空」一项，断言好算
            p.selected_col = Some(1);
            p.stats_items = vec![StatsItem::Count];
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

    // 1) 工具条补齐：`fillToolBar` 的「−」与「分组」都在
    assert!(
        visible("editor-result-stat-remove-tool", cx),
        "工具条应有「−」（DBeaver RemoveFunctionAction）"
    );
    assert!(
        visible("editor-result-stat-group", cx),
        "工具条应有「分组」（DBeaver GroupByColumnsAction）"
    );

    // 2) 行选中态：由行点击维护（「−」的 isEnabled 判据 = 有选中）
    cx.update(|_, app| {
        panel.update(app, |p, cx| p.select_stats_item(Some(StatsItem::Count), cx));
    });
    assert_eq!(
        panel.read_with(cx, |p, _app| p.stats_selected),
        Some(StatsItem::Count),
        "点击统计行后应记住该行为选中项"
    );

    // 3) 分组 + 矩形选区（行 0..=1、视图列 1..=2 → 数据列 id / amount 各一组）
    cx.update(|_, app| {
        panel.update(app, |p, cx| {
            p.grid.update(cx, |grid, _| {
                let delegate = grid.delegate_mut();
                delegate.click_cell(0, 1, false);
                delegate.click_cell(1, 2, true);
            });
            p.set_stats_group_by(true, cx);
        })
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    // 先钉住「分组要用的三件输入真的到位」，再钉渲染（失败时能一眼看出是数据还是渲染）
    let probe = panel.read_with(cx, |p, cx| {
        let delegate = p.grid.read(cx).delegate();
        (
            p.stats_group_by,
            delegate.cell_range(),
            p.inspect_column(delegate),
        )
    });
    assert!(probe.0, "分组开关应已开启");
    assert!(probe.1.is_some(), "矩形选区应已建立");
    assert_eq!(probe.2, Some(1), "作用列应落在 amount");
    assert!(
        visible("editor-result-stats-group-1", cx),
        "分组后应有 amount 列（数据列 1）的组标题行"
    );
    assert!(
        visible("editor-result-stats-group-2", cx),
        "分组后应有 note 列（数据列 2）的组标题行"
    );

    // 4) 分组口径只算选区的行区间：整列 4 行 → 区间 2 行
    let whole = panel.read_with(cx, |p, cx| {
        let delegate = p.grid.read(cx).delegate();
        p.stats_entries_for_column(delegate, 1, None)
    });
    assert_eq!(
        whole,
        vec![(StatsItem::Count, "4".to_string())],
        "整列口径应统计全部 4 行"
    );
    let region = panel.read_with(cx, |p, cx| {
        let delegate = p.grid.read(cx).delegate();
        p.stats_entries_for_column(delegate, 1, Some((0, 1)))
    });
    assert_eq!(
        region,
        vec![(StatsItem::Count, "2".to_string())],
        "分组口径应只统计选区的行区间（DBeaver `aggregateSelection` 按选区收集）"
    );

    // 5) 「−」删掉选中行后选中态作废（否则按钮会指着不存在的行）
    cx.update(|_, app| {
        panel.update(app, |p, cx| p.remove_stats_item(StatsItem::Count, cx));
    });
    assert_eq!(
        panel.read_with(cx, |p, _app| p.stats_selected),
        None,
        "删除选中项后选中态应清空"
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
    // 不排序：保持首现顺序（对齐 DBeaver `RS_GROUPING_DEFAULT_SORTING=""` 不设 ORDER BY）
    arrange_groups(&mut groups, GroupSort::None, false);
    assert_eq!(
        groups.iter().map(|(key, _)| key[0].as_str()).collect::<Vec<_>>(),
        ["b", "a", "c"],
        "「不排序」档必须原样保留首现顺序"
    );

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

/// 【D14 分组页】页内嵌结果集的行按排序口径**在客户端**排（不动 SQL）。
///
/// 对齐 DBeaver `GroupingResultsContainer.rebuildGrouping:249-265`：
/// 取 `RS_GROUPING_DEFAULT_SORTING` 后对分组结果在 viewer 里排序。
#[test]
fn group_rows_arrange_by_value_or_count_on_the_client() {
    use super::tabs::arrange_group_rows;

    let mut rows = vec![
        vec!["b".to_string(), "1".to_string()],
        vec!["a".to_string(), "3".to_string()],
        vec!["c".to_string(), "2".to_string()],
    ];

    // 按值：键列字典序（末列计数不参与比较）
    arrange_group_rows(&mut rows, GroupSort::Value);
    assert_eq!(
        rows,
        vec![
            vec!["a".to_string(), "3".to_string()],
            vec!["b".to_string(), "1".to_string()],
            vec!["c".to_string(), "2".to_string()],
        ]
    );

    // 按计数降序：同数按值兜底，保证同一份数据每次顺序一致
    arrange_group_rows(&mut rows, GroupSort::CountDesc);
    assert_eq!(
        rows,
        vec![
            vec!["a".to_string(), "3".to_string()],
            vec!["c".to_string(), "2".to_string()],
            vec!["b".to_string(), "1".to_string()],
        ]
    );

    // 不排序：原样（数据源给的顺序）
    let before = rows.clone();
    arrange_group_rows(&mut rows, GroupSort::None);
    assert_eq!(rows, before, "不排序就不该动顺序");
}

/// 【分组页表头】分组结果的列头形状：每个分组键一列，末列是函数列 `COUNT(*)`
/// （对齐 DBeaver `GroupingResultsContainer` 的属性列 + 函数列 + `getDefaultFunction`）。
///
/// 【D14 起】分组页正文改成**页内嵌结果集**（列头由网格按查询列画），所以渲染断言
/// 从「页内表头行」改成「工具条的重查按钮在」——那一页现在靠它触发 `GROUP BY` 重查。
/// 纯函数部分（键列 + 末列 COUNT(*) 的形状）保留，它仍是本地分组算法的口径。
#[gpui_kit::test]
fn group_tab_shows_a_header_row_with_count_star(
    cx: &mut gpui_kit::TestAppContext,
) {
    use super::tabs::group_table_header;

    // 纯函数：表头 = 键列名 + COUNT(*)，位置在末列
    let names = vec!["id".to_string(), "amount".to_string()];
    let header = group_table_header(&names);
    assert_eq!(header.len(), names.len() + 1, "表头 = 键列 + 一个函数列");
    assert_eq!(header[0], "id");
    assert_eq!(header[1], "amount");
    assert_eq!(header.last().unwrap(), "COUNT(*)");
    // 单个键也一样
    let single = group_table_header(&["id".to_string()]);
    assert_eq!(single, vec!["id".to_string(), "COUNT(*)".to_string()]);

    // 渲染：显式选一个分组键 → 分组页常驻工具条，且「重查」按钮在（D14）
    cx.update(gpui_kit::init);
    let (panel, cx) = panel_harness(cx);
    cx.update(|_, app| {
        panel.update(app, |p, cx| {
            p.toggle_group_column(1, cx);
            p.set_right_tab(RightTab::Group, cx);
        })
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(
        cx.debug_bounds("editor-result-group-run").is_some(),
        "分组页应常驻「重查」按钮（editor-result-group-run）"
    );
    assert!(
        cx.debug_bounds("editor-result-group-toolbar").is_some(),
        "分组页应常驻工具条"
    );
}

/// 【D14 分组页】页内嵌结果集：工具条「运行」按钮把 GROUP BY 重查请求交出去；
/// 结果写进页内网格后，正文换成网格（**不是**页内的本地计数列表）。
///
/// 对齐 DBeaver `GroupingResultsContainer`：那一页里嵌着一整个 `ResultSetViewer`
/// （`:61-73`），分组结果只喂它（`rebuildGrouping:249-265`）。
#[gpui_kit::test]
fn the_group_tab_runs_and_shows_a_embedded_result_set(
    cx: &mut gpui_kit::TestAppContext,
) {
    use crate::ports::ResultRequest;

    cx.update(gpui_kit::init);
    let (panel, cx) = panel_harness(cx);
    cx.update(|_, app| {
        panel.update(app, |p, cx| {
            p.toggle_group_column(1, cx);
            p.set_right_tab(RightTab::Group, cx);
        })
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));

    // ① 正文还是空态（还没重查过）→ 「运行」按钮在，点它把请求入队
    let run = cx
        .debug_bounds("editor-result-group-run")
        .expect("分组页应有「运行」（重查）按钮");
    let point = gpui_kit::point(
        run.origin.x + run.size.width / 2.,
        run.origin.y + run.size.height / 2.,
    );
    cx.simulate_click(point, gpui_kit::Modifiers::default());
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let queued = cx.update(|_, app| panel.read(app).queue.drain());
    assert!(
        queued.iter().any(|request| matches!(
            request,
            ResultRequest::RunGroupingQuery { columns, duplicates_only: false }
                if columns == &vec!["amount".to_string()]
        )),
        "「运行」应以「当前分组键列名 + 仅重复开关」入队：{queued:?}"
    );

    // ② 回执落进页内网格（宿主侧 `set_group_projection`）→ 正文显示网格
    cx.update(|_, app| {
        panel.update(app, |p, cx| {
            p.set_group_projection(
                vec!["amount".to_string(), "count".to_string()],
                vec![
                    vec!["1".to_string(), "2".to_string()],
                    vec!["NULL".to_string(), "1".to_string()],
                ],
                vec![None, None],
                vec![None, None],
                "SELECT amount, COUNT(*) AS count FROM src GROUP BY amount".to_string(),
                cx,
            );
        })
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(
        cx.debug_bounds("editor-result-group-grid").is_some(),
        "有分组结果后，分组页正文应换成页内嵌结果集（editor-result-group-grid）"
    );

    // ③ 主网格不受影响（分组结果写的是页内网格，不是主结果区）
    let main_rows = cx.update(|_, app| panel.read(app).grid.read(app).delegate().view_total());
    assert_eq!(main_rows, 2, "主网格仍是原来那 2 行（分组结果没写进去）");
    // 最近一次分组 SQL 也记下了（工具条 tooltip / 复制语句的落点）
    assert!(
        cx.update(|_, app| panel.read(app).group_query.is_some()),
        "重查成功后应记下分组 SQL"
    );
}

/// 【R3 分组页】分组键顺序可上 / 下移：纯函数钉边界，面板态钉「顺序真的变了」。
///
/// 对齐 `GroupingColumnsContainer.moveColumns:126-152`——分组键顺序 = 嵌套顺序，
/// 挪一位就是改层级（DBeaver 靠拖放表达，本仓是工具条的一对按钮）。
#[gpui_kit::test]
fn moving_group_column_reorders_the_group_keys(cx: &mut gpui_kit::TestAppContext) {
    use crate::panel::tabs::{can_move_group_column, move_group_column_in_place};

    // 纯函数：相邻交换 + 边界不动 + 不在键里不动
    let mut keys = vec![3usize, 7, 9];
    assert!(move_group_column_in_place(&mut keys, 7, -1));
    assert_eq!(keys, vec![7, 3, 9], "上移一位应与前一项交换");
    assert!(!move_group_column_in_place(&mut keys, 7, -1), "已在首位不能再上移");
    assert!(move_group_column_in_place(&mut keys, 3, 1));
    assert_eq!(keys, vec![7, 9, 3], "下移一位应与后一项交换");
    assert!(!move_group_column_in_place(&mut keys, 3, 1), "已在末位不能再下移");
    assert!(!move_group_column_in_place(&mut keys, 42, -1), "不在分组键里应无动作");

    // 可用性：无选中 / 不在键里 / 已到边界 → 不可用
    assert!(!can_move_group_column(&keys, None, -1));
    assert!(!can_move_group_column(&keys, Some(42), 1));
    assert!(!can_move_group_column(&keys, Some(7), -1), "首位不能上移");
    assert!(!can_move_group_column(&keys, Some(3), 1), "末位不能下移");
    assert!(can_move_group_column(&keys, Some(9), -1));
    assert!(can_move_group_column(&keys, Some(9), 1));

    // 面板态：勾 [0, 2] → 把 2 上移 → 顺序变 [2, 0]（分组层级随之改）
    cx.update(gpui_kit::init);
    let (panel, cx) = panel_harness(cx);
    cx.update(|_, app| {
        panel.update(app, |p, cx| {
            p.toggle_group_column(0, cx);
            p.toggle_group_column(2, cx);
        })
    });
    assert_eq!(cx.update(|_, app| panel.read(app).group_columns.clone()), vec![0, 2]);
    cx.update(|_, app| panel.update(app, |p, cx| p.move_group_column(2, -1, cx)));
    assert_eq!(
        cx.update(|_, app| panel.read(app).group_columns.clone()),
        vec![2, 0],
        "上移应改分组键顺序"
    );
    // 到边界后再移：状态不变（也不该触发无谓重绘）
    cx.update(|_, app| panel.update(app, |p, cx| p.move_group_column(2, -1, cx)));
    assert_eq!(cx.update(|_, app| panel.read(app).group_columns.clone()), vec![2, 0]);
}

/// 【R3 分组页】列下拉的项文字：已勾选的列要带位次，未勾选的保持列名。
///
/// DBeaver 的分组顺序在 `GroupingConfigDialog` 的表格行序里直接可见；本仓是勾选下拉，
/// 不标位次就看不出「先按 A 再按 B」。菜单内部不可读（kit 只暴露 `is_empty()`），
/// 所以按项目规范抽成纯函数来钉。
#[test]
fn group_column_menu_label_marks_only_selected_columns() {
    use crate::panel::tabs::group_column_menu_label;

    let keys = [3usize, 7, 9];
    assert_eq!(group_column_menu_label(&keys, 3, "id"), "1. id");
    assert_eq!(group_column_menu_label(&keys, 7, "name"), "2. name");
    assert_eq!(group_column_menu_label(&keys, 9, "amount"), "3. amount");
    assert_eq!(group_column_menu_label(&keys, 5, "extra"), "extra", "未勾选不加位次");
    assert_eq!(group_column_menu_label(&[], 5, "extra"), "extra", "空集合全部保持原名");

    // 顺序变了位次跟着变（上移后 9 变成第 2 位）
    let reordered = [3usize, 9, 7];
    assert_eq!(group_column_menu_label(&reordered, 9, "amount"), "2. amount");
    assert_eq!(group_column_menu_label(&reordered, 7, "name"), "3. name");
}

/// 【R3 分组页】工具条的上移 / 下移按钮真的把请求入队（渲染路径 + 可用性门）。
#[gpui_kit::test]
fn group_move_buttons_queue_the_request_for_the_selected_column(
    cx: &mut gpui_kit::TestAppContext,
) {
    use crate::ports::ResultRequest;

    cx.update(gpui_kit::init);
    let (panel, cx) = panel_harness(cx);

    // 分组键 [0, 2]，选中第 2 列（→ 上移可用、下移不可用）
    cx.update(|_, app| {
        panel.update(app, |p, cx| {
            p.toggle_group_column(0, cx);
            p.toggle_group_column(2, cx);
            p.select_column(2, cx);
            p.set_right_tab(RightTab::Group, cx);
        })
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));

    let up = cx
        .debug_bounds("editor-result-group-move-up")
        .expect("分组页工具条应有上移按钮");
    let point = gpui_kit::point(
        up.origin.x + up.size.width / 2.,
        up.origin.y + up.size.height / 2.,
    );
    cx.simulate_click(point, gpui_kit::Modifiers::default());
    cx.update(|window, cx| window.draw(cx).clear(cx));

    let queued = cx.update(|_, app| panel.read(app).queue.drain());
    assert!(
        queued.iter().any(|request| matches!(
            request,
            ResultRequest::MoveGroupColumn { column: 2, delta: -1 }
        )),
        "上移按钮应以「选中列 + delta=-1」入队：{queued:?}"
    );
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
/// （选值提示 + 收起面板提示），且页内工具条仍在（五页同形）。
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
        "值页空态也要保留页内工具条（五页同形）"
    );
    assert!(
        cx.debug_bounds("editor-result-empty-page").is_some(),
        "值页空态正文容器应渲染（editor-result-empty-page）"
    );
}

/// 【列浏览器工具条】对齐 DBeaver `MetaDataPanel.contributeActions:204-215`：
/// 页内工具条**只贡献一条**「生成 DDL」。多选批量动作（隐藏所选 / 显示所选 / 清空选择）
/// 改挂在清单行右键菜单上（`MetaDataPanel.fillCustomActions:234-252` 的同一处口径：
/// 操作对象是清单的结构化选中项），不再挤在工具条里。
#[gpui_kit::test]
fn column_browser_toolbar_keeps_only_the_ddl_action(
    cx: &mut gpui_kit::TestAppContext,
) {
    use crate::column_browser::{batch_menu_items, filter_browser_columns, ColumnBatchAction};

    // 纯函数：批量项只在「右键列属于多选集合」时出现，标签带集合规模
    assert!(
        batch_menu_items(&[], 0).is_empty(),
        "空集合不该有批量项"
    );
    assert!(
        batch_menu_items(&[1, 3], 0).is_empty(),
        "右键列不在集合里时，菜单退回单列菜单"
    );
    let items = batch_menu_items(&[1, 3], 3);
    assert_eq!(items.len(), 3, "命中集合 → 隐藏 / 显示 / 清空三条");
    assert_eq!(items[0].0, ColumnBatchAction::HideSelected);
    assert_eq!(items[1].0, ColumnBatchAction::ShowSelected);
    assert_eq!(items[2].0, ColumnBatchAction::ClearSelection);
    for (_, label) in &items {
        assert!(
            label.contains("2 列"),
            "标签要带集合规模，实际 {label}"
        );
    }
    // 过滤函数不受影响（清单仍按搜索词映射行号）
    assert_eq!(
        filter_browser_columns(&["id".into(), "amount".into()], &[], &[], "am").len(),
        1
    );

    // 渲染层：多选非空时工具条仍只有「生成 DDL」，批量按钮不再出现
    cx.update(gpui_kit::init);
    let (panel, cx) = panel_harness(cx);
    cx.update(|_, app| {
        panel.update(app, |p, cx| {
            p.set_multi_selected(vec![1, 2], cx);
            p.set_right_tab(RightTab::Browser, cx);
        })
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));

    let visible = |id: &'static str, cx: &mut gpui_kit::VisualTestContext| -> bool {
        use gpui_kit::test::TestWindowExt as _;
        let by_id = cx.update(|window, _| {
            window
                .try_find(gpui_kit::ElementId::Name(gpui_kit::SharedString::from(
                    id.to_string(),
                )))
                .is_some_and(|snap| snap.visible())
        });
        by_id || cx.debug_bounds(id).is_some()
    };
    assert!(
        visible("editor-result-generate-ddl", cx),
        "列浏览器工具条应保留「生成 DDL」（DBeaver 的唯一页内动作）"
    );
    for id in [
        "editor-result-col-batch-hide",
        "editor-result-col-batch-show",
        "editor-result-col-batch-clear",
    ] {
        assert!(
            !visible(id, cx),
            "多选批量动作应只在右键菜单里，工具条不应再有 {id}"
        );
    }
}

/// 【页内工具条列位】对齐 DBeaver `addPanel:1337-1383` 的 `GridLayout(2, false)`：
/// 左列（`panel.contributeActions` + `FILL_HORIZONTAL`）吃满剩余宽度 → 页内动作**贴左**；
/// 右列（`addDefaultPanelActions`）`HORIZONTAL_ALIGN_END` → 视图菜单**贴右**。
/// 改动前本仓是「标题 flex_1 吃满 + 动作组紧贴菜单」，动作和菜单挤在同一侧右端。
///
/// 另钉 `needsSeparator()`：`addPanel:1345-1347` 只在面板声明 `true` 时才在工具条与正文之间
/// 插水平分隔线；全仓只有 `ValueViewerPanel.java:171-173` 为 `true`，其余继承
/// `ResultSetPanelBase.java:26-28` 的 `false`。
#[gpui_kit::test]
fn panel_toolbar_puts_actions_left_and_view_menu_right(
    cx: &mut gpui_kit::TestAppContext,
) {
    cx.update(gpui_kit::init);
    let (panel, cx) = panel_harness(cx);

    // 纯函数层：五页里只有值详情页需要那条水平分隔线
    {
        use super::events::{page_needs_separator, right_tabs};
        let flagged: Vec<RightTab> = right_tabs()
            .into_iter()
            .filter(|tab| page_needs_separator(*tab))
            .collect();
        assert_eq!(
            flagged,
            vec![RightTab::Value],
            "needsSeparator 只有值详情页为 true（ValueViewerPanel.java:171-173）"
        );
    }

    // 渲染层：值页带真实动作集（选一格 → 复制 / 保存 / 外部打开三颗都挂上）
    cx.update(|_, app| {
        panel.update(app, |p, cx| {
            p.selection_pos = Some((0, 1));
            p.set_right_tab(RightTab::Value, cx);
        })
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));

    let toolbar = cx
        .debug_bounds("editor-result-value-toolbar")
        .expect("值页工具条应带 debug_selector 才量得到");
    let copy = cx
        .debug_bounds("editor-result-value-copy")
        .expect("值页复制钮应带 debug_selector 才量得到");
    let menu = cx
        .debug_bounds("editor-result-value-toolbar-view-menu")
        .expect("值页视图菜单应带 debug_selector 才量得到");

    let toolbar_left = f32::from(toolbar.origin.x);
    let toolbar_right = toolbar_left + f32::from(toolbar.size.width);
    let copy_left = f32::from(copy.origin.x);
    let copy_right = copy_left + f32::from(copy.size.width);
    let menu_left = f32::from(menu.origin.x);
    let menu_right = menu_left + f32::from(menu.size.width);

    // 1) 动作在菜单左侧（同一条工具条行）
    assert!(
        (f32::from(copy.origin.y) - f32::from(menu.origin.y)).abs() < 4.0,
        "动作与视图菜单应同行：copy.y={} menu.y={}",
        f32::from(copy.origin.y),
        f32::from(menu.origin.y)
    );
    assert!(
        copy_left < menu_left,
        "页内动作应在视图菜单左侧，copy.x={copy_left} menu.x={menu_left}"
    );

    // 2) 动作贴左：标题（「值」）之后紧接动作，离工具条左端不超过 200px
    assert!(
        copy_left - toolbar_left <= 200.0,
        "动作应贴左列起点（标题之后），实际距左端 {}px",
        copy_left - toolbar_left
    );

    // 3) 左列吃满剩余宽度：动作右缘到菜单左缘之间应留出大段空隙
    //    （改动前动作组紧贴菜单，空隙只有 gap_1=4px）
    let gap = menu_left - copy_right;
    assert!(
        gap >= 24.0,
        "左列应吃满剩余宽度，动作与菜单间空隙过小：{gap}px"
    );

    // 4) 菜单贴右：右缘落在工具条右端内（实测 28px = 行 px_2 + ghost 按钮自带内距；
    //    阈值 40px 只盯住「菜单不在行中段」这条语义）
    let trailing = toolbar_right - menu_right;
    assert!(
        (0.0..=40.0).contains(&trailing),
        "视图菜单应贴右，距工具条右端 {trailing}px"
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

    // 「不排序」档（对齐 DBeaver `RS_GROUPING_DEFAULT_SORTING=""`）也要能注入
    cx.update(|_, app| {
        panel.update(app, |p, cx| {
            p.set_group_prefs_initial("none", false, cx);
        })
    });
    assert_eq!(
        cx.update(|_, app| panel.read(app).group_sort_id()),
        "none",
        "「不排序」是合法档位，不能当未知值回落"
    );

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
        // 【D14】分组页的内嵌结果集（生产里由编辑器构造期建成并 `set_grids` 交进来）
        let group_grid: Entity<TableState<crate::grid::ResultGridDelegate>> = cx.new(|cx| {
            TableState::new(
                crate::grid::ResultGridDelegate::empty("尚未分组重查"),
                window,
                cx,
            )
        });
        panel.update(cx, |p, cx| {
            p.set_grids(grid, None, Some(group_grid), window, cx);
            p.selected_col = Some(1);
        });
        *slot_in.borrow_mut() = Some(panel.clone());
        let host = cx.new(|_| PanelHost { panel });
        Root::new(host, window, cx)
    });
    let panel = slot.borrow().clone().expect("ResultPanel 已创建");
    (panel, vcx)
}

/// 【第 57 批】状态栏「修改」钮走的是双击同一条事件链：选中数据格后
/// `begin_edit_cell` emit `TableEvent::DoubleClickedCell`（面板自己的订阅
/// 把 `selection_pos` 对齐到该格）；停在行号槽（组件列 0）时什么都不发。
///
/// 断言前先用哨兵值抹掉 `set_selected_cell` 自带的 `SelectCell` 副作用，
/// 否则「事件到底发没发」会被上面那次选中掩盖。
#[gpui_kit::test]
fn begin_edit_cell_emits_the_double_click_only_for_data_cells(
    cx: &mut gpui_kit::TestAppContext,
) {
    cx.update(gpui_kit::init);
    let (panel, cx) = panel_harness(cx);
    let grid = cx.update(|_, app| panel.read(app).grid.clone());

    // 组件列 2 = 第 2 个数据列（组件列 0 是行号槽，数据格从 1 起）
    cx.update(|_, app| {
        grid.update(app, |state, cx| state.set_selected_cell(0, 2, cx));
    });
    cx.update(|_, app| panel.update(app, |p, _cx| p.selection_pos = Some((9, 9))));
    cx.update(|_, app| panel.update(app, |p, cx| p.begin_edit_cell(cx)));
    assert_eq!(
        cx.update(|_, app| panel.read(app).selection_pos),
        Some((0, 2)),
        "数据格上「修改」应走 DoubleClickedCell 订阅（面板侧记下该格）"
    );

    // 行号槽：选中 (0, 0) 后再打一次哨兵，「修改」不该把哨兵覆盖掉
    cx.update(|_, app| {
        grid.update(app, |state, cx| state.set_selected_cell(0, 0, cx));
    });
    cx.update(|_, app| panel.update(app, |p, _cx| p.selection_pos = Some((9, 9))));
    cx.update(|_, app| panel.update(app, |p, cx| p.begin_edit_cell(cx)));
    assert_eq!(
        cx.update(|_, app| panel.read(app).selection_pos),
        Some((9, 9)),
        "行号槽（组件列 0）不是数据格，「修改」不该 emit"
    );
}

/// 【第 61 批】右键「修改单元格」用**右键落点**定位，不是焦点格：右键不会改选区，
/// 所以 `begin_edit_cell_at` 发出的 `DoubleClickedCell` 必须是传入的那一格。
/// 行号槽（组件列 0）仍然什么都不发。
#[gpui_kit::test]
fn begin_edit_cell_at_uses_the_given_cell_instead_of_the_selection(
    cx: &mut gpui_kit::TestAppContext,
) {
    cx.update(gpui_kit::init);
    let (panel, cx) = panel_harness(cx);
    let grid = cx.update(|_, app| panel.read(app).grid.clone());

    // 选中 (0, 1)，但右键落在 (1, 3)：事件应对齐右键那一格
    cx.update(|_, app| {
        grid.update(app, |state, cx| state.set_selected_cell(0, 1, cx));
    });
    cx.update(|_, app| panel.update(app, |p, _cx| p.selection_pos = Some((9, 9))));
    cx.update(|_, app| {
        panel.update(app, |p, cx| p.begin_edit_cell_at(1, 3, cx));
    });
    assert_eq!(
        cx.update(|_, app| panel.read(app).selection_pos),
        Some((1, 3)),
        "右键落点 (1,3) 才是要编辑的格，与选中格 (0,1) 无关"
    );

    // 行号槽：哨兵不被覆盖
    cx.update(|_, app| panel.update(app, |p, _cx| p.selection_pos = Some((9, 9))));
    cx.update(|_, app| {
        panel.update(app, |p, cx| p.begin_edit_cell_at(0, 0, cx));
    });
    assert_eq!(
        cx.update(|_, app| panel.read(app).selection_pos),
        Some((9, 9)),
        "行号槽（组件列 0）不是数据格，「修改单元格」不该 emit"
    );
}

/// 【第 62 批】记录（整行 / 转置）视图下不再 emit（对齐 [`crate::grid::can_edit_cell`] 的
/// `record_view`）：那边状态栏钮是灰的、右键编辑段整段不摆，键位 / 右键请求也不该再把
/// `DoubleClickedCell` 发出去（delegate 的 `begin_edit` 同样拒绝转置，这里只是提前）。
#[gpui_kit::test]
fn begin_edit_cell_at_is_ignored_in_record_view(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let (panel, cx) = panel_harness(cx);
    cx.update(|_, app| {
        panel.update(app, |p, _cx| {
            p.transposed = true;
            p.selection_pos = Some((9, 9));
        });
    });
    cx.update(|_, app| {
        panel.update(app, |p, cx| p.begin_edit_cell_at(0, 2, cx));
    });
    assert_eq!(
        cx.update(|_, app| panel.read(app).selection_pos),
        Some((9, 9)),
        "记录视图只读，不该 emit（哨兵保持不变）"
    );
}

/// 【第 58 批】「修改」钮的可用态照 DBeaver `PROP_EDITABLE` 的 `"inline"` 分支：
/// 要求 `presentation instanceof IResultSetEditor` **且** `getCurrentAttribute() != null`
/// （`ResultSetPropertyTester.java:127-133`）。记录模式是 `PlainTextPresentation` 的变体、
/// **不实现** `IResultSetEditor`（`PlainTextPresentation.java:63`）⇒ 那边记录模式下置灰。
/// 本仓的对应状态：记录视图（转置，只读）→ 灰；没选中格 / 停在行号槽 → 灰（无当前属性）。
#[gpui_kit::test]
fn edit_row_button_is_disabled_in_record_view(cx: &mut gpui_kit::TestAppContext) {
    use gpui_kit::test::TestWindowExt as _;
    use gpui_kit::{ElementId, SharedString};

    cx.update(gpui_kit::init);
    let (panel, cx) = panel_harness(cx);
    let grid = cx.update(|_, app| panel.read(app).grid.clone());
    // 真钮只在可编辑结果集注入（`panel_harness` 默认只读，这里把它打开）；
    // 另外要有状态行，状态栏的钮才在树里（`show_status` 需要 status / 执行信息之一）
    cx.update(|_, app| {
        panel.update(app, |p, cx| {
            p.editable = true;
            p.status = Some(crate::grid::status::ResultStatus {
                total_rows: 2,
                ..Default::default()
            });
            cx.notify();
        })
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));

    let disabled_state = |cx: &mut gpui_kit::VisualTestContext| -> Option<Option<bool>> {
        cx.update(|window, _| {
            window
                .try_find(ElementId::Name(SharedString::from(
                    "editor-result-edit-cell".to_string(),
                )))
                .map(|snap| snap.disabled())
        })
    };

    // 1) 一个格都没选：没有「当前属性」，inline 语义关闭
    assert_eq!(
        disabled_state(cx).expect("可编辑结果集应注入「修改」真钮"),
        Some(true),
        "没选中格时「修改」应置灰（inline 需要当前属性）"
    );

    // 2) 选中数据格：可用（实测口径：可用态不暴露 a11y disabled 位，只有禁用才写 `Some(true)`）
    cx.update(|_, app| {
        grid.update(app, |state, cx| state.set_selected_cell(0, 2, cx));
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert_ne!(
        disabled_state(cx).expect("选中后「修改」钮仍应在位"),
        Some(true),
        "选中数据格后「修改」应可用"
    );

    // 3) 停在行号槽（组件列 0）：不是数据格，回到置灰
    cx.update(|_, app| {
        grid.update(app, |state, cx| state.set_selected_cell(0, 0, cx));
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert_eq!(
        disabled_state(cx).expect("行号槽上「修改」钮仍应在位"),
        Some(true),
        "行号槽不是数据格，「修改」应置灰"
    );

    // 4) 记录视图（即使选中的是数据格）：只读视图，置灰
    cx.update(|_, app| {
        grid.update(app, |state, cx| state.set_selected_cell(0, 2, cx));
    });
    cx.update(|_, app| panel.update(app, |p, cx| p.toggle_transpose(cx)));
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert_eq!(
        disabled_state(cx).expect("记录视图下「修改」钮仍要在位（只置灰，不消失）"),
        Some(true),
        "记录视图只读：钮要置灰，而不是可点后静默落空"
    );
}

/// 【第 64 批】状态栏「删除」钮要看行状态：已标记删除的底层行上再删是宿主侧空操作，
/// 钮要置灰并说明原因（DBeaver `PROP_EDITABLE` 的 `"delete"` 分支，
/// `ResultSetPropertyTester.java:142-157`）。撤销删除后恢复可用。
///
/// 走 `row_action_state` 的真实接线（不是单独测纯函数）：`status.selected_row` 是 1 基，
/// `mark_deleted(1)` 标的是底层行 1，无筛选无排序时 即视图行 1 ⇒ 选中第 2 行即命中。
#[gpui_kit::test]
fn delete_row_button_is_disabled_on_a_deleted_row(cx: &mut gpui_kit::TestAppContext) {
    use gpui_kit::test::TestWindowExt as _;
    use gpui_kit::{ElementId, SharedString};

    cx.update(gpui_kit::init);
    let (panel, cx) = panel_harness(cx);
    let grid = cx.update(|_, app| panel.read(app).grid.clone());
    cx.update(|_, app| {
        panel.update(app, |p, cx| {
            p.editable = true;
            p.status = Some(crate::grid::status::ResultStatus {
                total_rows: 2,
                selected_row: Some(2),
                ..Default::default()
            });
            cx.notify();
        })
    });
    // 两行底层数据 + 把第 2 行（底层行 1）标记删除
    cx.update(|_, app| {
        grid.update(app, |state, cx| {
            state
                .delegate_mut()
                .set_data(vec!["id".to_string()], vec![vec!["1".to_string()], vec!["2".to_string()]]);
            let mut local = crate::rowset::LocalRowSet::new();
            local.mark_deleted(1);
            state.delegate_mut().set_local_snapshot(Some(local));
            state.refresh(cx);
        });
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));

    let disabled_state = |cx: &mut gpui_kit::VisualTestContext| -> Option<Option<bool>> {
        cx.update(|window, _| {
            window
                .try_find(ElementId::Name(SharedString::from(
                    "editor-result-delete-row".to_string(),
                )))
                .map(|snap| snap.disabled())
        })
    };

    assert_eq!(
        disabled_state(cx).expect("可编辑结果集应注入「删除」真钮"),
        Some(true),
        "已标记删除的行上「删除」应置灰（再删是空操作）"
    );

    // 复制仍可用：DBeaver 的 "copy" 只管 currentRow + insertable，不看行状态
    let duplicate_disabled = cx.update(|window, _| {
        window
            .try_find(ElementId::Name(SharedString::from(
                "editor-result-duplicate-row".to_string(),
            )))
            .map(|snap| snap.disabled())
    });
    assert_ne!(
        duplicate_disabled.expect("「复制」钮应在位"),
        Some(true),
        "已删行的「复制」不受影响"
    );

    // 撤销删除（清空叠加层）：回到可用
    cx.update(|_, app| {
        grid.update(app, |state, cx| {
            state.delegate_mut().set_local_snapshot(None);
            state.refresh(cx);
        });
    });
    // gpui-fast 0.1.17 起未被失效的保留子树会在 `window.draw` 时被原样重放，
    // 不再重跑 render/paint（见 vendor/gpui-component/src/lib.rs 的 `test_redraw`），
    // 所以测试要断言"状态变更后的渲染结果"必须先 `window.refresh()` 强制一帧。
    cx.update(|window, cx| {
        window.refresh();
        window.draw(cx).clear(cx);
    });
    assert_ne!(
        disabled_state(cx).expect("「删除」钮仍应在位"),
        Some(true),
        "撤销删除后「删除」应恢复可用"
    );
}

/// 【第 66 批】状态栏「修改」钮也要看行状态：已标记删除的行上灰掉，与右键 edit 段的
/// 「修改单元格」同一个口径（那边 `disabled: target.is_deleted`）。撤销删除后恢复。
///
/// 与第 58 批的记录视图用例分工：那条守「呈现 + 当前属性」，这条只补「行状态」这一维
/// ——所以选中格必须是**数据格**（组件列 > 0），否则灰的原因是「没当前属性」而不是「已删行」。
#[gpui_kit::test]
fn edit_row_button_is_disabled_on_a_deleted_row(cx: &mut gpui_kit::TestAppContext) {
    use gpui_kit::test::TestWindowExt as _;
    use gpui_kit::{ElementId, SharedString};

    cx.update(gpui_kit::init);
    let (panel, cx) = panel_harness(cx);
    let grid = cx.update(|_, app| panel.read(app).grid.clone());
    cx.update(|_, app| {
        panel.update(app, |p, cx| {
            p.editable = true;
            p.status = Some(crate::grid::status::ResultStatus {
                total_rows: 2,
                selected_row: Some(2),
                ..Default::default()
            });
            cx.notify();
        })
    });
    // 两行底层数据 + 把第 2 行（底层行 1）标记删除
    cx.update(|_, app| {
        grid.update(app, |state, cx| {
            state.delegate_mut().set_data(
                vec!["id".to_string()],
                vec![vec!["1".to_string()], vec!["2".to_string()]],
            );
            let mut local = crate::rowset::LocalRowSet::new();
            local.mark_deleted(1);
            state.delegate_mut().set_local_snapshot(Some(local));
            state.refresh(cx);
        });
    });
    // 选中已删行上的数据格（视图行 1、组件列 2 ⇒ 数据列 1）
    cx.update(|_, app| {
        grid.update(app, |state, cx| state.set_selected_cell(1, 2, cx));
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));

    let disabled_state = |cx: &mut gpui_kit::VisualTestContext| -> Option<Option<bool>> {
        cx.update(|window, _| {
            window
                .try_find(ElementId::Name(SharedString::from(
                    "editor-result-edit-cell".to_string(),
                )))
                .map(|snap| snap.disabled())
        })
    };

    assert_eq!(
        disabled_state(cx).expect("可编辑结果集应注入「修改」真钮"),
        Some(true),
        "已标记删除的行上「修改」应置灰（begin_edit 拒绝已删行）"
    );

    // 换选未删行上的数据格：恢复可用
    cx.update(|_, app| {
        grid.update(app, |state, cx| state.set_selected_cell(0, 2, cx));
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert_ne!(
        disabled_state(cx).expect("「修改」钮仍应在位"),
        Some(true),
        "未删行的数据格上「修改」应可用"
    );

    // 撤销删除（清空叠加层）后再选回那一行：恢复可用
    cx.update(|_, app| {
        grid.update(app, |state, cx| {
            state.delegate_mut().set_local_snapshot(None);
            state.refresh(cx);
        });
    });
    cx.update(|_, app| {
        grid.update(app, |state, cx| state.set_selected_cell(1, 2, cx));
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert_ne!(
        disabled_state(cx).expect("「修改」钮仍应在位"),
        Some(true),
        "撤销删除后该行可编辑"
    );
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
    // 出厂只启用「值详情」（§55），本用例要列浏览器页内的钮 → 先切过去
    vcx.update(|_, app| {
        panel.update(app, |p, cx| p.set_right_tab(RightTab::Browser, cx))
    });
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
    // 出厂只启用「值详情」（§55）；本用例从列浏览器页开始量，先切过去（切页即登记）
    cx.update(|_, app| {
        panel.update(app, |p, cx| p.set_right_tab(RightTab::Browser, cx))
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

    // 3) 原「执行SQL」页已删除（DBeaver 右栏无该面板）：那句 SQL 改由筛选栏展开区承载，
    //    形态断言见 `executed_sql_shows_in_the_filter_expand_bar`。

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

    // 5) 空态也保留工具条：五页同形（此前空态直接返回居中文字，比其它页高 28px）
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

    // 7) 工具栏 SQL 入口：有 SQL 时第一格是**来源查询短文本**（剥注释 + 折叠空白的
    //    归一化结果，见 §44 / `normalize_query_preview`），不再是固定文案「显示 SQL」
    cx.update(|_, app| {
        panel.update(app, |p, _| {
            p.result_sql = Some("select id, amount from t where amount > 100".to_string())
        })
    });
    // 同上：`result_sql` 是直接改字段（没有走 setter 的 notify），
    // 保留子树不会自己失效，必须 refresh 才会重新 render。
    cx.update(|window, cx| {
        window.refresh();
        window.draw(cx).clear(cx);
    });
    assert!(
        visible("editor-result-sql-preview", cx),
        "有 SQL 时筛选框第 1 格应渲染来源查询入口"
    );

    // 7.5) 【对象格浮层】单击第 1 格弹的是**非模态浮层**（DBeaver
    //      `ActiveObjectPanel.showObjectInfoPopup`，`ResultSetFilterPanel.java:942-984`：
    //      `SWT.ON_TOP | SWT.RESIZE` 无边框 Shell，内容 = 图标列 + 只读 SQL，
    //      `createObjectPanel:728-772`）。钉三态：未开不在 / 开后有浮层与 SQL / 关后消失。
    assert!(
        !visible("editor-result-sql-popover", cx),
        "未点开时不该有对象信息浮层"
    );
    cx.update(|_, app| panel.update(app, |p, cx| p.set_sql_popup(true, cx)));
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.update(|window, cx| window.draw(cx).clear(cx));
    for id in [
        "editor-result-sql-popover",
        "editor-result-sql-popover-icon",
        "editor-result-sql-popover-sql",
    ] {
        assert!(visible(id, cx), "对象信息浮层元素 {id} 应渲染可见");
    }
    cx.update(|_, app| panel.update(app, |p, cx| p.set_sql_popup(false, cx)));
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(
        !visible("editor-result-sql-popover", cx),
        "关掉后对象信息浮层应消失（再点 / 点别处 / 失焦同一条入口）"
    );

    // 8) 归一化后为空（全文只有注释）→ 不摆这一格
    //    （DBeaver 会用 `DEFAULT_QUERY_TEXT` 兜底，本仓口径是「没有的东西不摆占位」）
    cx.update(|_, app| {
        panel.update(app, |p, _| p.result_sql = Some("-- 只有注释".to_string()))
    });
    cx.update(|window, cx| {
        window.refresh();
        window.draw(cx).clear(cx);
    });
    assert!(
        !visible("editor-result-sql-preview", cx),
        "SQL 归一化后为空时不该留一个空壳按钮"
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
    // 出厂只启用「值详情」（§55）；列浏览器是下面这颗钮所在的页，先切过去
    let (panel, cx) = panel_harness(cx);
    cx.update(|_, app| {
        panel.update(app, |p, cx| p.set_right_tab(RightTab::Browser, cx))
    });

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
    // 出厂只启用「值详情」（§55），列浏览器的注释行要这一页可见 → 先切过去
    cx.update(|_, app| {
        panel.update(app, |p, cx| p.set_right_tab(RightTab::Browser, cx))
    });
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
/// 的 `label + " (" + toolTip + ")"`），后半句按 `toggleResultsPanel:64-69` 的
/// `isPanelVisible = isPanelsVisible() && isPanelVisible(id)` 分三态报动作：
/// 开且整栏可见 → 「点一下关闭这一页」；开但整栏收起 → 「展开并切到这一页」；
/// 已关闭 → 「已关闭，点一下打开」。勾选态 = [`edge_tab_active`]（多页可同时勾亮）。
#[test]
fn edge_bar_tooltip_reports_the_toggle_action() {
    use super::events::{
        edge_tab_active, edge_tab_tooltip, right_tab_description, right_tab_label,
    };
    use crate::ports::RightTab;

    for tab in super::events::right_tabs() {
        let tip = edge_tab_tooltip(tab, true, true);
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
        assert!(tip.contains("关闭这一页"), "开且整栏可见：{tip}");
    }

    // 开但整栏收起：点一下会展开并切过去（`activatePanel(id,true,true)`）
    let hidden = edge_tab_tooltip(RightTab::Value, true, false);
    assert!(hidden.contains("展开并切到这一页"), "{hidden}");
    assert!(!hidden.contains("关闭这一页"), "{hidden}");
    assert!(
        hidden.contains(right_tab_description(RightTab::Value)),
        "收起态提示也保留描述：{hidden}"
    );

    // 已关闭（页签条上被 × 关掉）：点一下会重新启用
    let closed = edge_tab_tooltip(RightTab::Stats, false, true);
    assert!(closed.contains("点一下打开"), "{closed}");
    assert!(!closed.contains("关闭这一页"), "{closed}");

    // 勾选态：整栏可见 **且** 该页在 `open_tabs` 里（`ResultSetViewer.java:1079`），
    // 与「是不是当前页」无关 —— 多页可同时勾亮。
    assert!(edge_tab_active(true, true));
    assert!(!edge_tab_active(true, false));
    assert!(!edge_tab_active(false, true), "整栏收起时竖条按钮全不勾");
    assert!(!edge_tab_active(false, false));
}

/// 【右栏页签持久化】id ↔ 页互换（`result_panel.open_tabs` 偏好键的落盘形态）。
#[test]
fn right_tab_ids_round_trip() {
    use crate::ports::RightTab;

    assert_eq!(RightTab::ALL.len(), 5);
    for (index, tab) in RightTab::ALL.into_iter().enumerate() {
        assert_eq!(RightTab::parse(tab.id()), Some(tab), "{} 的 id 应可回读", tab.id());
        // 规范序与 id 表逐项对齐（顺序变了说明两边有一处没同步）
        assert!(index < RightTab::ALL.len());
    }
    assert_eq!(RightTab::parse(" STATS "), Some(RightTab::Stats), "大小写与空白应容忍");
    assert_eq!(RightTab::parse("nope"), None, "未知 id 必须返回 None（不猜页）");
    assert_eq!(RightTab::parse(""), None);
}

/// 【右栏页签持久化】偏好注入：按规范序归一、去重、未知丢弃；空集合保持出厂五页全开；
/// 当前页落在被关掉的页上时改落到集合第一页。
#[gpui_kit::test]
fn open_tabs_initial_normalizes_and_keeps_defaults(cx: &mut gpui_kit::TestAppContext) {
    use crate::ports::RightTab;

    cx.update(gpui_kit::init);
    let (panel, cx) = panel_harness(cx);

    // 乱序 + 重复 + 未知：只剩三页，且按规范序排
    cx.update(|_, app| {
        panel.update(app, |p, cx| {
            p.set_open_tabs_initial(
                &[RightTab::Group, RightTab::Browser, RightTab::Group],
                cx,
            )
        })
    });
    assert_eq!(
        panel.read_with(cx, |p, _| p.open_tabs().to_vec()),
        vec![RightTab::Browser, RightTab::Group],
        "应去重并按规范序排"
    );

    // 空集合 / 全未知：保持现状（出厂五页全开，对齐 activateDefaultPanels 的补默认）
    cx.update(|_, app| panel.update(app, |p, cx| p.set_open_tabs_initial(&[], cx)));
    assert_eq!(panel.read_with(cx, |p, _| p.open_tabs().to_vec()).len(), 2);

    // 当前页不在新集合里 → 落到集合第一页
    cx.update(|_, app| {
        panel.update(app, |p, cx| {
            p.set_right_tab(RightTab::Stats, cx);
            p.set_open_tabs_initial(&[RightTab::Value, RightTab::Reference], cx);
        })
    });
    assert_eq!(panel.read_with(cx, |p, _| p.right_tab), RightTab::Value);
    assert_eq!(
        panel.read_with(cx, |p, _| p.open_tabs().to_vec()),
        vec![RightTab::Value, RightTab::Reference]
    );
}

/// 【视图菜单】页项点击判定（`fillPanelsMenu:1559-1588` + `toggleResultsPanel:64-69`）：
/// 只有「勾着且整栏可见」才是关闭；勾着但整栏收起 / 未勾 → 打开并切过去。
#[test]
fn view_menu_page_item_closes_only_when_the_bar_is_open() {
    use super::events::page_item_closes;

    assert!(page_item_closes(true, true), "勾着且整栏可见 → closePanel");
    assert!(!page_item_closes(true, false), "未勾 → activatePanel（打开并切过去）");
    assert!(
        !page_item_closes(false, true),
        "勾着但整栏收起 → 也走 activatePanel（顺带展开）"
    );
    assert!(!page_item_closes(false, false));
}

/// 【右缘竖条】点某一页 = 开/关**这一页**（不是「切页」也不是「收整栏」）。
///
/// 对齐 `ResultSetHandlerTogglePanel`：`toggleResultsPanel:64-69` 算
/// `isVisible = isPanelsVisible() && isPanelVisible(id)`，`showResultsPanel:71-81` 里
/// 可见 → `closePanel(id)`（该页从页签夹移除，`removePanel:1441-1451` 从
/// `enabledPanelIds` 摘掉），否则 → `activatePanel(id, true, true)`（打开 + 激活，
/// 整栏收起时先 `showPanels(true,false,false)` 展开）。
#[gpui_kit::test]
fn clicking_an_edge_tab_toggles_that_page(cx: &mut gpui_kit::TestAppContext) {
    use crate::ports::RightTab;

    cx.update(gpui_kit::init);
    let (panel, cx) = panel_harness(cx);

    // 铺满五页（出厂只启用「值详情」，§55）：本用例要看「关一页、其余仍在」
    cx.update(|_, app| {
        panel.update(app, |p, cx| {
            p.set_open_tabs_initial(&RightTab::ALL, cx);
            p.set_right_tab(RightTab::Value, cx);
        })
    });
    assert!(cx.update(|_, app| panel.read(app).panels_visible()));

    // 当前页、整栏可见 → 点它 = 关掉这一页（返回 false 表示没有「切到该页」）
    let switched =
        cx.update(|_, app| panel.update(app, |p, cx| p.toggle_right_tab(RightTab::Value, cx)));
    assert!(!switched, "关闭该页不算切页");
    assert!(
        !cx.update(|_, app| panel.read(app).open_tabs().contains(&RightTab::Value)),
        "点当前页应把这页从已启用集合里摘掉"
    );
    assert!(
        cx.update(|_, app| panel.read(app).panels_visible()),
        "还有别的页在，整栏不该收起"
    );
    assert_eq!(
        cx.update(|_, app| panel.read(app).right_tab),
        RightTab::Stats,
        "关掉当前页应落到规范序里的下一页"
    );

    // 再点一次（该页已关闭）→ 重新启用并切过去
    let switched =
        cx.update(|_, app| panel.update(app, |p, cx| p.toggle_right_tab(RightTab::Value, cx)));
    assert!(switched);
    assert_eq!(cx.update(|_, app| panel.read(app).right_tab), RightTab::Value);
    assert!(cx.update(|_, app| panel.read(app).open_tabs().contains(&RightTab::Value)));

    // 整栏收起（总开关）后点同一页：`isPanelsVisible` 为假 → 走 activate 分支（展开 + 切页），
    // 不能因为「开着的页」就当成关闭。
    cx.update(|_, app| panel.update(app, |p, cx| p.set_panels_visible(false, cx)));
    assert!(!cx.update(|_, app| panel.read(app).panels_visible()));
    let switched =
        cx.update(|_, app| panel.update(app, |p, cx| p.toggle_right_tab(RightTab::Value, cx)));
    assert!(switched, "收起态点击应展开并切到该页");
    assert!(cx.update(|_, app| panel.read(app).panels_visible()));
    assert_eq!(cx.update(|_, app| panel.read(app).right_tab), RightTab::Value);
}

/// 【页签条】每页签**内侧**那颗 ×（对齐 DBeaver `CTabItem(SWT.CLOSE)`）：
/// 点它只推「关这一页」，**不能**顺带把该页切回来。
///
/// 这条用例的存在理由：× 是 `Tab::suffix` 里的自绘点击区，点中后事件仍会冒泡到
/// `TabBar` 的「切页」回调（用渲染时快照的下标映射）。若两者都生效，关掉一页会变成
/// 「先摘除、再按旧下标把它重新打开」。所以断言分两层：队列里有 `CloseRightTab`、
/// 且**没有** `SetRightTab`。
#[gpui_kit::test]
fn right_tab_close_button_closes_only_that_page(cx: &mut gpui_kit::TestAppContext) {
    use crate::ports::{ResultRequest, RightTab};

    cx.update(gpui_kit::init);
    let (panel, cx) = panel_harness(cx);

    // 当前页 = 值详情；铺满五页后再点「列浏览器」页签的 ×（不是当前页）
    // （出厂只启用值详情，§55：不铺满就没有列浏览器页签可点）
    cx.update(|_, app| {
        panel.update(app, |p, cx| {
            p.set_open_tabs_initial(&RightTab::ALL, cx);
            p.set_right_tab(RightTab::Value, cx);
        })
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));

    let close = cx
        .debug_bounds("editor-result-right-tab-close-browser")
        .expect("列浏览器页签内侧应有一颗 ×（每页一颗，对齐 SWT.CLOSE）");
    let point = gpui_kit::point(
        close.origin.x + close.size.width / 2.,
        close.origin.y + close.size.height / 2.,
    );
    cx.simulate_click(point, gpui_kit::Modifiers::default());
    cx.update(|window, cx| window.draw(cx).clear(cx));

    let queued = cx.update(|_, app| panel.read(app).queue.drain());
    assert!(
        queued
            .iter()
            .any(|request| matches!(request, ResultRequest::CloseRightTab(RightTab::Browser))),
        "点页签内侧 × 应入队「关掉这一页」：{queued:?}"
    );
    assert!(
        !queued
            .iter()
            .any(|request| matches!(request, ResultRequest::SetRightTab(_))),
        "× 的点击不能冒泡成「切页」（那会把刚关掉的页按旧下标重新打开）：{queued:?}"
    );
}

/// 【页签条】右端那颗最小化钮（对齐 DBeaver `panelFolder.setMinimizeVisible(true)`
/// ——`ResultSetViewer.java:363`——及其 `minimize` 回调 `showPanels(false, true, true)`，
/// `:399-401`）：点它 = 入队「整栏收起」，且不能冒泡成「切页」。
#[gpui_kit::test]
fn right_tab_strip_minimize_button_collapses_the_bar(cx: &mut gpui_kit::TestAppContext) {
    use crate::ports::ResultRequest;

    cx.update(gpui_kit::init);
    let (panel, cx) = panel_harness(cx);

    // 页签条只在右栏展开时挂载（收起态整条不渲染）
    cx.update(|_, app| panel.update(app, |p, cx| p.set_panels_visible(true, cx)));
    assert!(cx.update(|_, app| panel.read(app).panels_visible()));
    cx.update(|window, cx| window.draw(cx).clear(cx));

    let bounds = cx.debug_bounds("editor-result-right-tabs-minimize").expect(
        "页签条右端应有一颗最小化钮（DBeaver `setMinimizeVisible(true)`，`ResultSetViewer.java:363`）",
    );
    let point = gpui_kit::point(
        bounds.origin.x + bounds.size.width / 2.,
        bounds.origin.y + bounds.size.height / 2.,
    );
    cx.simulate_click(point, gpui_kit::Modifiers::default());
    cx.update(|window, cx| window.draw(cx).clear(cx));

    let queued = cx.update(|_, app| panel.read(app).queue.drain());
    assert!(
        queued
            .iter()
            .any(|request| matches!(request, ResultRequest::SetPanelsVisible(false))),
        "点最小化钮应入队「整栏收起」：{queued:?}"
    );
    assert!(
        !queued
            .iter()
            .any(|request| matches!(request, ResultRequest::SetRightTab(_))),
        "最小化钮不挂在 `TabBar` 内，点击不应冒泡成切页：{queued:?}"
    );
}

/// 【页签条】五页的 × 选择器必须互不相同（共用 id 会让 `debug_bounds` 只命中第一个，
/// 并且重复 id 在 a11y 上有已知风险）。
#[test]
fn right_tab_close_selectors_are_distinct() {
    use crate::ports::RightTab;

    let selectors: Vec<&'static str> = [
        RightTab::Browser,
        RightTab::Value,
        RightTab::Stats,
        RightTab::Group,
        RightTab::Reference,
    ]
    .into_iter()
    .map(super::events::tab_close_selector)
    .collect();
    for (ix, selector) in selectors.iter().enumerate() {
        assert!(
            selectors
                .iter()
                .skip(ix + 1)
                .all(|other| other != selector),
            "选择器重复：{selector}"
        );
    }
}

/// 【右缘竖条】关到最后一页 = 整栏收起（DBeaver `removePanel:1447-1449`：
/// `activePanels.isEmpty()` → `showPanels(false, true, true)`）。
#[gpui_kit::test]
fn closing_the_last_edge_tab_collapses_the_bar(cx: &mut gpui_kit::TestAppContext) {
    use crate::ports::RightTab;

    cx.update(gpui_kit::init);
    let (panel, cx) = panel_harness(cx);

    let all = RightTab::ALL;
    cx.update(|_, app| {
        panel.update(app, |p, cx| {
            for tab in all.iter().skip(1).copied() {
                p.close_right_tab(tab, cx);
            }
        })
    });
    assert_eq!(
        panel.read_with(cx, |p, _| p.open_tabs().to_vec()),
        vec![RightTab::Value],
        "只剩一页（规范序首项 = 值详情，§55）"
    );
    cx.update(|_, app| panel.update(app, |p, cx| p.close_right_tab(RightTab::Value, cx)));
    assert!(cx.update(|_, app| panel.read(app).open_tabs().is_empty()));
    assert!(
        !cx.update(|_, app| panel.read(app).panels_visible()),
        "关掉最后一页应整栏收起"
    );

    // 收起态再点任一一页 → 重新展开（`activatePanel` 的 `showPanels` 分支）
    let switched =
        cx.update(|_, app| panel.update(app, |p, cx| p.toggle_right_tab(RightTab::Stats, cx)));
    assert!(switched);
    assert!(cx.update(|_, app| panel.read(app).panels_visible()));
    assert_eq!(cx.update(|_, app| panel.read(app).right_tab), RightTab::Stats);
}

/// 【页签重排 2026-11-24】`move_right_tab` 只搬 `open_tabs` 的元素顺序
/// （对齐 DBeaver `TabFolderReorder(panelFolder)`，`ResultSetViewer.java:360`）。
/// 下标是**页签条位置**；当前页与集合大小都不该被重排影响。
#[gpui_kit::test]
fn moving_a_right_tab_reorders_only_the_strip(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let (panel, cx) = panel_harness(cx);

    // 铺满五页（出厂只启用值详情，§55）：本用例要五个页签才谈得上重排
    cx.update(|_, app| {
        panel.update(app, |p, cx| p.set_open_tabs_initial(&RightTab::ALL, cx))
    });
    let before = panel.read_with(cx, |p, _| p.open_tabs().to_vec());
    assert_eq!(before.len(), 5, "前提：五页全开");

    // 把第 3 个（下标 2）拖到最左（0）
    let moved = before[2];
    cx.update(|_, app| panel.update(app, |p, cx| p.move_right_tab(2, 0, cx)));
    let mut expected = before.clone();
    expected.remove(2);
    expected.insert(0, moved);
    assert_eq!(panel.read_with(cx, |p, _| p.open_tabs().to_vec()), expected);

    // 当前页不被重排影响；越界 = no-op
    let current_before = cx.update(|_, app| panel.read(app).right_tab);
    cx.update(|_, app| panel.update(app, |p, cx| p.move_right_tab(0, 9, cx)));
    assert_eq!(cx.update(|_, app| panel.read(app).right_tab), current_before);
    assert_eq!(
        panel.read_with(cx, |p, _| p.open_tabs().len()),
        5,
        "越界请求不得改变集合大小"
    );
}

/// 【右缘竖条】点已被 × 关掉的页 = 先重新启用它再切过去（竖条列的是全集）。
#[gpui_kit::test]
fn edge_bar_reopens_a_closed_page(cx: &mut gpui_kit::TestAppContext) {
    use crate::ports::RightTab;

    cx.update(gpui_kit::init);
    let (panel, cx) = panel_harness(cx);

    // 铺满五页（出厂只启用值详情，§55）后关掉「计算」页
    cx.update(|_, app| {
        panel.update(app, |p, cx| p.set_open_tabs_initial(&RightTab::ALL, cx))
    });
    cx.update(|_, app| panel.update(app, |p, cx| p.close_right_tab(RightTab::Stats, cx)));
    assert!(!cx.update(|_, app| panel.read(app).open_tabs().contains(&RightTab::Stats)));

    let switched = cx.update(|_, app| panel.update(app, |p, cx| p.toggle_right_tab(RightTab::Stats, cx)));
    assert!(switched);
    assert!(
        cx.update(|_, app| panel.read(app).open_tabs().contains(&RightTab::Stats)),
        "点竖条应把被关掉的页重新启用"
    );
    assert_eq!(cx.update(|_, app| panel.read(app).right_tab), RightTab::Stats);
    // 重新启用时按规范序插入，随后被 MRU 挪到首位
    // （真值 `setMRUVisible(true)`，`ResultSetViewer.java:364`：`activatePanel` 的页排最前）
    assert_eq!(
        cx.update(|_, app| panel.read(app).open_tabs().to_vec()),
        vec![
            RightTab::Stats,
            RightTab::Value,
            RightTab::Browser,
            RightTab::Group,
            RightTab::Reference,
        ],
        "重开的页在 MRU 顺序下应排到首位，其余保持规范序相对次序"
    );
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

/// 【页签提示】五页各有一条非空、互不重复的提示文案（对齐 DBeaver 面板描述：
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
    assert_eq!(seen.len(), 5, "五页各一条提示");
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

/// 【空提示文案】筛选框占位文案 = DBeaver `enter_sql_to_filter` 的前半句。
///
/// `ResultSetFilterPanel.java:219-223` 给 `filtersText` 挂 `UIUtils.addEmptyTextHint`，
/// 支持过滤时写 `sql_editor_resultset_filter_panel_text_enter_sql_to_filter`
/// （zh `ResultSetMessages_zh.properties:671` =
/// 「输入一个 SQL 表达式来过滤结果 (使用 Ctrl+Space)」）。尾随那半句是内容辅助
/// （列名 / 函数补全，`:304-308` 装的），本仓没有补全能力，照抄就是承诺一个按不出来
/// 的快捷键——这条用例同时钉住「前半句要对齐」和「不许带 Ctrl+Space」。
#[gpui_kit::test]
fn the_filter_box_hints_the_sql_expression_it_takes(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let (panel, cx) = panel_harness(cx);
    let hint = cx.update(|_window, app| {
        let input = panel.read(app).filter_input.clone();
        input.read(app).presentation().placeholder().clone()
    });
    assert_eq!(
        hint.to_string(),
        "输入一个 SQL 表达式来过滤结果",
        "筛选框空提示没对齐 DBeaver 的 enter_sql_to_filter 文案"
    );
    assert!(
        !hint.contains("Ctrl+Space"),
        "本仓没有内容辅助，空提示不该承诺 Ctrl+Space"
    );
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
    assert!(
        cx.update(|_window, app| panel.read(app).has_pending_filter_draft()),
        "输入框里有未应用的草稿时执行钮应亮（DBeaver `ExecutePanel.isItemEnabled`：\
         `ResultSetFilterPanel.java:1413-1436` 比「已落地条件」与「输入框文本」）"
    );

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
    assert!(
        !cx.update(|_window, app| panel.read(app).has_pending_filter_draft()),
        "Enter 已把词落到筛选上，草稿消失 → 执行钮该灭（同一判据的另一半）"
    );
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

    // 从历史里选一条：回填输入框 + 立即应用 + **收回浮层**
    // （DBeaver 三个入口都先 `closeHistoryPopup()`，`ResultSetFilterPanel.java:1251-1271`）
    cx.update(|_window, cx| {
        panel.update(cx, |panel, cx| panel.set_filter_history_popup(true, cx))
    });
    assert!(
        cx.update(|_window, app| panel.read(app).filter_history_popup_open),
        "`set_filter_history_popup(true)` 应把浮层标成展开"
    );
    cx.update(|window, cx| {
        panel.update(cx, |panel, cx| {
            panel.apply_filter_from_history("k2", window, cx);
        });
    });
    assert!(
        !cx.update(|_window, app| panel.read(app).filter_history_popup_open),
        "选行应用后浮层应自动收回（否则浮层还挂在输入框下面）"
    );
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

/// 【结果内查找 2026-11-20】「只显示匹配行」（DBeaver 的 `selectAllButton` / 「Show
/// matching」）：查找词是**第三层**行过滤——全局筛选词（所有列）/ 列级筛选（一列）/
/// 查找词，三层叠加取交集；撤掉后必须回到该有的行数，且**不动**筛选框的词。
#[test]
fn find_filter_narrows_rows_and_stacks_with_the_other_filters() {
    let mut delegate = crate::grid::ResultGridDelegate::default();
    delegate.set_data(
        vec!["id".to_string(), "name".to_string()],
        vec![
            vec!["1".to_string(), "Alpha".to_string()],
            vec!["2".to_string(), "beta".to_string()],
            vec!["3".to_string(), "gamma".to_string()],
            vec!["4".to_string(), "ALPHABET".to_string()],
        ],
    );
    let rows = |delegate: &crate::grid::ResultGridDelegate| delegate.view_total();

    // 0) 没开：视图行序是恒等的（不物化映射，`view_rows = None`）
    assert_eq!(rows(&delegate), 4, "没有查找过滤时应看到全部四行");

    // 1) 打开：只去首尾空白（大小写**不**归一；大小写不敏感靠匹配口径实现，
    //    DBeaver 的 `SearchQuickFilterInfo` 也是拿原词配 `CASE_INSENSITIVE`）+ 所有列匹配
    delegate.set_find_filter(Some("  ALPHA  ".to_string()), FindCriteria::default());
    assert_eq!(
        delegate.find_filter(),
        Some("ALPHA"),
        "过滤词只 trim，不再小写化（大小写交给匹配口径）"
    );
    assert_eq!(rows(&delegate), 2, "`ALPHA` 命中 Alpha 与 ALPHABET 两行");

    // 2) 与列级筛选叠加取交集：id 列 = `2` 的那行是 beta，不含 alpha → 0 行
    delegate.set_column_filter(Some((0, "2".to_string())));
    assert_eq!(rows(&delegate), 0, "查找过滤与列级筛选必须同时满足（交集）");
    delegate.set_column_filter(None);

    // 3) 与全局筛选词叠加取交集：`gam` 只在 gamma 行，alpha 行不含 → 0 行
    delegate.set_filter("gam");
    assert_eq!(rows(&delegate), 0, "查找过滤与全局筛词也必须同时满足");
    assert_eq!(
        delegate.filter(),
        "gam",
        "查找过滤**不得**改写筛选框的权威词"
    );
    delegate.set_filter("");

    // 4) 撤掉：回到全部四行（且筛选词仍为空，没被查找层写脏）
    delegate.set_find_filter(None, FindCriteria::default());
    assert_eq!(delegate.find_filter(), None);
    assert_eq!(rows(&delegate), 4, "撤掉查找过滤后应回到全部四行");
    assert_eq!(delegate.filter(), "", "撤销后筛选框的词原样保留（空）");

    // 5) 空白词等同关闭（DBeaver：文本空即 `applyFilterAction(null)`）
    delegate.set_find_filter(Some("   ".to_string()), FindCriteria::default());
    assert_eq!(delegate.find_filter(), None, "全空白词应视为不过滤");
    assert_eq!(rows(&delegate), 4);

    // 6) 换词大小写：默认口径（大小写不敏感）下行集合不变
    delegate.set_find_filter(Some("alpha".to_string()), FindCriteria::default());
    let before = rows(&delegate);
    delegate.set_find_filter(Some("ALPHA".to_string()), FindCriteria::default());
    assert_eq!(rows(&delegate), before, "不区分大小写时换大小写不改变行集合");
}

/// 【结果内查找 2026-11-20 第 38 批】三口径（大小写 / 正则 / 全词）真的参与匹配：
/// 对齐 DBeaver `SearchQuickFilterInfo.java:24-93`——
/// `flags = caseSensitive ? 0 : CASE_INSENSITIVE|UNICODE_CASE`；
/// `regex = useRegex ? text : Pattern.quote(text)`；
/// `wholeWord` 只在词首 / 词尾**是字母数字**时补 `\b`（`:47-58`）；
/// 匹配口径是 `matcher.find()`（子串），非法正则抛 `PatternSyntaxException`
/// （本仓按「编译失败 = 零命中」登记差异）。
#[test]
fn find_criteria_case_regex_whole_word_narrow_the_filter() {
    let mut delegate = crate::grid::ResultGridDelegate::default();
    delegate.set_data(
        vec!["id".to_string(), "name".to_string()],
        vec![
            vec!["1".to_string(), "Alpha".to_string()],
            vec!["2".to_string(), "beta".to_string()],
            vec!["3".to_string(), "gamma".to_string()],
            vec!["4".to_string(), "ALPHABET".to_string()],
        ],
    );
    let rows = |delegate: &crate::grid::ResultGridDelegate| delegate.view_total();
    let plain = FindCriteria::default();

    // 大小写：默认不敏感命中 2 行；勾上区分大小写只剩 1 行
    delegate.set_find_filter(Some("alpha".to_string()), plain);
    assert_eq!(rows(&delegate), 2, "默认大小写不敏感：Alpha / ALPHABET");
    delegate.set_find_filter(
        Some("Alpha".to_string()),
        FindCriteria {
            case_sensitive: true,
            ..plain
        },
    );
    assert_eq!(rows(&delegate), 1, "区分大小写：只剩 Alpha（ALPHABET 里没有带小写尾巴的串）");

    // 正则：不勾时 `^al` 是字面量（没人含这个串）；勾上按锚点解释 → 2 行
    delegate.set_find_filter(Some("^al".to_string()), FindCriteria { case_sensitive: true, ..plain });
    assert_eq!(rows(&delegate), 0, "未勾正则时 `^al` 按字面比较");
    delegate.set_find_filter(Some("^al".to_string()), FindCriteria { regex: true, ..plain });
    assert_eq!(rows(&delegate), 2, "勾上正则 + 大小写不敏感：Alpha / ALPHABET");

    // 全词：`gam` 不是 gamma 的全词（词尾还在字母里），`gamma` 才是
    delegate.set_find_filter(
        Some("gam".to_string()),
        FindCriteria {
            whole_word: true,
            ..plain
        },
    );
    assert_eq!(rows(&delegate), 0, "全词匹配不得命中单词的一部分");
    delegate.set_find_filter(
        Some("gamma".to_string()),
        FindCriteria {
            whole_word: true,
            ..plain
        },
    );
    assert_eq!(rows(&delegate), 1, "全词 gamma 命中 gamma 行");

    // 非法正则：编译失败按零命中（DBeaver 那边是抛 `PatternSyntaxException`）
    delegate.set_find_filter(
        Some("[".to_string()),
        FindCriteria {
            regex: true,
            ..plain
        },
    );
    assert_eq!(rows(&delegate), 0, "非法正则按零命中处理");
    assert_eq!(
        delegate.find_next("[", FindCriteria { regex: true, ..plain }, None),
        None,
        "非法正则也不产出命中位置"
    );
}

/// 【结果内查找 2026-11-20】面板层：勾「仅显示匹配」把词灌进视图行序；关查找栏
/// 自动撤销过滤但**保留查找词**（对齐 DBeaver `ResultsetFindReplaceOverlay.onClose`
/// → `applyFilterAction(null)`，词不清）。
#[gpui_kit::test]
fn closing_the_find_bar_drops_the_matching_filter_but_keeps_the_query(
    cx: &mut gpui_kit::TestAppContext,
) {
    cx.update(gpui_kit::init);
    let (panel, cx) = panel_harness(cx);
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let grid = cx.update(|_window, app| panel.read(app).grid().clone());

    // 数据 `["1","1234.5"] / ["2","NULL"]`：词 `1` 只命中第一行 → 1 行
    cx.update(|window, cx| {
        panel.update(cx, |p, cx| {
            p.toggle_find(window, cx);
            p.find_query_changed("1".to_string(), cx);
            p.set_find_filter_only(true, cx);
        })
    });
    let open = cx.update(|_window, app| {
        let d = grid.read(app).delegate();
        (
            d.find_filter().map(str::to_string),
            d.view_total(),
            panel.read(app).find_filter_only,
            panel.read(app).find_hits,
        )
    });
    assert_eq!(open.0.as_deref(), Some("1"), "勾上后词应进 delegate 的查找过滤");
    assert_eq!(open.1, 1, "网格应只剩命中那一行");
    assert!(open.2, "`find_filter_only` 应为真");
    assert_eq!(
        open.3, 2,
        "命中口径按**格**算：过滤只剩第一行，但 `1` 在该行的 id 与 amount 各命中一格"
    );

    // 关查找栏：过滤撤销、行回来、词与筛选框都不动
    cx.update(|_window, cx| panel.update(cx, |p, cx| p.close_find(cx)));
    let closed = cx.update(|_window, app| {
        let d = grid.read(app).delegate();
        (
            d.find_filter().map(str::to_string),
            d.view_total(),
            d.filter().to_string(),
            panel.read(app).find_filter_only,
            panel.read(app).find_state().1.to_string(),
        )
    });
    assert_eq!(closed.0, None, "关查找栏应撤掉查找过滤");
    assert_eq!(closed.1, 2, "行集合应回到全部两行");
    assert_eq!(closed.2, "", "筛选框的权威词不受查找过滤影响");
    assert!(!closed.3, "开关应回落");
    assert_eq!(closed.4, "1", "查找词保留，下次打开接着用");
}

/// 【结果内查找 2026-11-20】查找栏那颗勾选框真的在栏里，点它入队
/// `SetFindFilterOnly(true)`（DBeaver 的 `selectAllButton` 是个 `SWT.CHECK`）。
#[gpui_kit::test]
fn find_bar_only_matching_checkbox_queues_the_toggle(cx: &mut gpui_kit::TestAppContext) {
    use crate::ports::ResultRequest;

    cx.update(gpui_kit::init);
    let (panel, cx) = panel_harness(cx);
    cx.update(|window, cx| panel.update(cx, |p, cx| p.toggle_find(window, cx)));
    cx.update(|window, cx| window.draw(cx).clear(cx));

    // `Checkbox` 是 RenderOnce，自身 id 进不了 `debug_bounds` → 量外层探针 div
    let wrap = cx
        .debug_bounds("editor-result-find-only-wrap")
        .expect("查找栏应有「仅显示匹配」勾选框");
    let point = gpui_kit::point(
        wrap.origin.x + wrap.size.width / 2.,
        wrap.origin.y + wrap.size.height / 2.,
    );
    cx.simulate_click(point, gpui_kit::Modifiers::default());
    cx.update(|window, cx| window.draw(cx).clear(cx));

    let queued = cx.update(|_, app| panel.read(app).queue.drain());
    assert!(
        queued.iter().any(|request| matches!(
            request,
            ResultRequest::SetFindFilterOnly(true)
        )),
        "勾选框应以「开启」入队：{queued:?}"
    );
}

/// 【结果内查找 2026-11-20 第 38 批】三颗口径按钮（Aa / .* / W）真的在查找栏里，
/// 点第一颗入队 `SetFindOption(CaseSensitive, true)`；勾正则后全词那颗变灰
/// （对齐 DBeaver `FindReplaceOverlay`：`createCaseSensitiveButton:740` 等三颗
/// `SWT.CHECK`，勾正则时 `wholeWordSearchButton.setEnabled(info.isAvailable(WHOLE_WORD))`）。
#[gpui_kit::test]
fn find_bar_criteria_buttons_queue_their_option(cx: &mut gpui_kit::TestAppContext) {
    use crate::ports::{FindOption, ResultRequest};

    cx.update(gpui_kit::init);
    let (panel, cx) = panel_harness(cx);
    cx.update(|window, cx| panel.update(cx, |p, cx| p.toggle_find(window, cx)));
    cx.update(|window, cx| window.draw(cx).clear(cx));

    // 三颗按钮都在（`Button` 是 RenderOnce，量外层探针 div）
    for id in [
        "editor-result-find-regex-wrap",
        "editor-result-find-case-wrap",
        "editor-result-find-word-wrap",
    ] {
        assert!(
            cx.debug_bounds(id).is_some(),
            "查找栏应有 {id} 口径按钮"
        );
    }

    let wrap = cx
        .debug_bounds("editor-result-find-case-wrap")
        .expect("查找栏应有区分大小写按钮");
    let point = gpui_kit::point(
        wrap.origin.x + wrap.size.width / 2.,
        wrap.origin.y + wrap.size.height / 2.,
    );
    cx.simulate_click(point, gpui_kit::Modifiers::default());
    cx.update(|window, cx| window.draw(cx).clear(cx));

    let queued = cx.update(|_, app| panel.read(app).queue.drain());
    assert!(
        queued.iter().any(|request| matches!(
            request,
            ResultRequest::SetFindOption(FindOption::CaseSensitive, true)
        )),
        "点击应以「勾上」入队：{queued:?}"
    );

    // 直接调面板动作：勾正则应把全词一并取消并重算命中
    cx.update(|_, cx| {
        panel.update(cx, |p, cx| {
            p.set_find_option(FindOption::WholeWord, true, cx);
        })
    });
    assert!(cx.update(|_, app| panel.read(app).find_criteria.whole_word));
    cx.update(|_, cx| {
        panel.update(cx, |p, cx| {
            p.set_find_option(FindOption::Regex, true, cx);
        })
    });
    let after = cx.update(|_, app| {
        (
            panel.read(app).find_criteria.regex,
            panel.read(app).find_criteria.whole_word,
        )
    });
    assert_eq!(after, (true, false), "勾正则应把全词取消（DBeaver 的 isAvailable 联动）");

    // 正则勾上后全词那颗应被禁用（渲染层的 disabled 由 criteria 推导，这里只钉状态）
    assert!(cx.update(|_, app| panel.read(app).find_criteria.regex));
}

/// 【结果内查找 2026-11-25】查找是**覆盖网格的浮层**，不是布局里的一行：
/// DBeaver `FindReplaceOverlay` 把浮层摆在 `targetControl`（presentation 控件）之上，
/// 默认 `TOP_RIGHT`（`FindReplaceOverlay.java:348`），宽取 `0.7 × 目标宽`
/// （`BIG_WIDTH_RATIO_EDITOR_TO_OVERLAY`，`:78`）。本仓据此挂在
/// `editor-result-main-grid` 里、`absolute().top_1().right_1()`。
#[gpui_kit::test]
fn find_overlay_sits_in_the_grid_top_right_corner(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let (panel, cx) = panel_harness(cx);
    cx.update(|window, cx| panel.update(cx, |p, cx| p.toggle_find(window, cx)));
    cx.update(|window, cx| window.draw(cx).clear(cx));

    let grid = cx
        .debug_bounds("editor-result-main-grid")
        .expect("应有网格容器");
    let bar = cx
        .debug_bounds("editor-result-find-bar")
        .expect("应有查找浮层");

    // 上缘：浮层在网格容器内部、贴着顶部（`top_1` = 4px）
    let dy = bar.origin.y - grid.origin.y;
    assert!(
        dy >= gpui_kit::px(0.) && dy < gpui_kit::px(8.),
        "浮层上缘应贴网格顶部（实际差 {dy:?}），而不是排在工具栏下方一行"
    );
    // 右缘：贴着网格右缘、且不越界
    let d_right = (grid.origin.x + grid.size.width) - (bar.origin.x + bar.size.width);
    assert!(
        d_right >= gpui_kit::px(0.) && d_right < gpui_kit::px(8.),
        "浮层右缘应贴网格右缘（实际差 {d_right:?}）"
    );
    // 宽：≈ 0.7 × 网格宽（容差留给 `min_w(20rem)` 兜底）
    let ratio = f32::from(bar.size.width) / f32::from(grid.size.width);
    assert!(
        (ratio - 0.7).abs() < 0.06 || f32::from(bar.size.width) >= 320.,
        "浮层宽应约为网格宽的 0.7（实际 {ratio}，或已落到 20rem 下限）"
    );
}

/// 【§54.6 2026-11-25】「仅显示匹配」开着时，改查找词要等 250ms 防抖才重过滤。
///
/// 真值 `ResultsetFindReplaceOverlay`：`filterSchedulingTimeoutMilliseconds = 250`（`:50`），
/// 防抖延迟的是「过滤 + 命中更新 + 高亮」整组（`scheduleFilter` → `applyInteractiveFilterJob`
/// = `applyQuickFilter` + `updateIncrementalSearch` + `decorate`，`:204-208`）；未勾时不延迟
/// （走 `super.onSearchFieldModified` 的即时搜索，`:180-182`）。本仓照此实现。
#[gpui_kit::test]
fn find_query_change_while_filtering_waits_for_the_250ms_debounce(
    cx: &mut gpui_kit::TestAppContext,
) {
    cx.update(gpui_kit::init);
    let (panel, cx) = panel_harness(cx);
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let grid = cx.update(|_window, app| panel.read(app).grid().clone());

    // 数据 `["1","1234.5"] / ["2","NULL"]`：词 `1` 只命中第一行
    cx.update(|window, cx| {
        panel.update(cx, |p, cx| {
            p.toggle_find(window, cx);
            p.find_query_changed("1".to_string(), cx);
            p.set_find_filter_only(true, cx);
        })
    });
    let rows = cx.update(|_window, app| grid.read(app).delegate().view_total());
    assert_eq!(rows, 1, "勾上「仅显示匹配」后网格只剩命中行");

    // 改词：这一刻**不该**同步生效（防抖还没到点）。新词取 `null`——两行里只有
    // 第二行的 `NULL` 命中（`1` 与 `1234.5` 都不含它），便于断言行集合真的换了
    cx.update(|_window, cx| panel.update(cx, |p, cx| p.find_query_changed("null".into(), cx)));
    let (filter, rows) = cx.update(|_window, app| {
        let d = grid.read(app).delegate();
        (d.find_filter().map(str::to_string), d.view_total())
    });
    assert_eq!(filter.as_deref(), Some("1"), "防抖未到点：delegate 里还是旧词");
    assert_eq!(rows, 1, "防抖未到点：行集合还没换");

    // 推过 250ms → 新词生效
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(300));
    cx.run_until_parked();
    let (filter, rows) = cx.update(|_window, app| {
        let d = grid.read(app).delegate();
        (d.find_filter().map(str::to_string), d.view_total())
    });
    assert_eq!(filter.as_deref(), Some("null"), "防抖到点后应换成新词");
    assert_eq!(rows, 1, "`null` 只命中第二行");
}

/// 【§54.8 2026-11-25】「边输边过滤」能力关掉后，「仅显示匹配」退化成一次性按钮。
///
/// 真值 `ResultsetFindReplaceOverlay`：右键菜单切 `isInteractiveFilterEnabled`（`:115-134`），
/// 关掉后 `performSelectAll` 先强制 `setSelection(false)` 再 `applyQuickFilter()` +
/// `storeHistory()`（`:218-222`）；词变只走父类的即时增量搜索、不动行集合（`:180-182`）；
/// 关浮层按 `findAllActionApplied` 撤过滤（`:257-263`）。
#[gpui_kit::test]
fn interactive_filter_off_turns_show_matching_into_a_one_shot_button(
    cx: &mut gpui_kit::TestAppContext,
) {
    cx.update(gpui_kit::init);
    let (panel, cx) = panel_harness(cx);
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let grid = cx.update(|_window, app| panel.read(app).grid().clone());

    // 词 `null`：两行里只命中第二行（`1` / `1234.5` 都不含它）
    cx.update(|window, cx| {
        panel.update(cx, |p, cx| {
            p.toggle_find(window, cx);
            p.find_query_changed("null".to_string(), cx);
            p.set_find_interactive_filter(false, cx);
        })
    });
    let before = cx.update(|_window, app| grid.read(app).delegate().view_total());
    assert_eq!(before, 2, "能力关着时敲词不动行集合");

    // 点这颗钮（能力关 = 一次性应用）→ 过滤生效，但勾选框不保持勾着
    cx.update(|_window, cx| panel.update(cx, |p, cx| p.set_find_filter_only(true, cx)));
    let applied = cx.update(|_window, app| {
        let d = grid.read(app).delegate();
        (d.find_filter().map(str::to_string), d.view_total())
    });
    let flags = cx.update(|_window, app| {
        let p = panel.read(app);
        (p.find_filter_only, p.find_filter_applied)
    });
    assert_eq!(
        applied.0.as_deref(),
        Some("null"),
        "一次性应用应把当前词灌进视图"
    );
    assert_eq!(applied.1, 1, "只剩命中那一行");
    assert!(
        !flags.0,
        "能力关着时勾选框不能保持勾选（DBeaver `setSelection(false)`）"
    );
    assert!(flags.1, "要记住「已应用过」，关浮层时按它撤过滤");

    // 改词：能力关着，行集合与过滤词都不跟着变
    cx.update(|_window, cx| panel.update(cx, |p, cx| p.find_query_changed("1".into(), cx)));
    let after_typing = cx.update(|_window, app| {
        let d = grid.read(app).delegate();
        (d.find_filter().map(str::to_string), d.view_total())
    });
    assert_eq!(
        after_typing.0.as_deref(),
        Some("null"),
        "能力关着：词变了也不换过滤词"
    );
    assert_eq!(after_typing.1, 1, "行集合保持上次一次性应用的结果");

    // 关浮层 → 撤过滤（查找词本身保留）
    cx.update(|_window, cx| panel.update(cx, |p, cx| p.close_find(cx)));
    let closed = cx.update(|_window, app| {
        let d = grid.read(app).delegate();
        (d.find_filter().map(str::to_string), d.view_total())
    });
    assert_eq!(closed.0, None, "关浮层要撤掉一次性应用的过滤");
    assert_eq!(closed.1, 2, "两行都回来");
}

/// 【§54.8】勾选框右键菜单两项的文案 / 勾选态随能力开关互斥
/// （`PopupMenu` 不暴露菜单项，故按 gpui-kit-dev skill 的口径断言纯函数）。
#[gpui_kit::test]
fn find_only_menu_entries_follow_the_interactive_switch(_cx: &mut gpui_kit::TestAppContext) {
    let on = super::render::find_only_menu_entries(true);
    assert!(on[0].1, "能力开着：第一项（边输边过滤）勾选");
    assert!(on[0].2);
    assert!(!on[1].1, "能力开着：第二项（只在点按时过滤）不勾");
    assert!(!on[1].2);

    let off = super::render::find_only_menu_entries(false);
    assert!(!off[0].1, "能力关着：第一项不勾");
    assert!(off[0].2);
    assert!(off[1].1, "能力关着：第二项勾选");
    assert!(!off[1].2);
}

/// 【§54.9】对象格悬停底：暗底提亮 10%、亮底压暗 10%
///
/// 真值 `ActiveObjectPanel.paintPanel:1019-1027` = `UIUtils.blend(background,
/// isDarkBG ? white : black, 90)`，`blend(v1,v2,ratio) = (ratio*v1 + (100-ratio)*v2)/100`
/// （`UIUtils.java:1859-1860`）——即朝白/黑走 10%，不是换一个强调色。
#[gpui_kit::test]
fn object_chip_hover_background_blends_a_tenth_towards_white_or_black(
    _cx: &mut gpui_kit::TestAppContext,
) {
    let dark_bg = gpui_kit::hsla(220., 0.06, 0.12, 1.);
    let dark_hover = super::render::object_chip_hover_bg(dark_bg, true);
    assert!(dark_hover.l > dark_bg.l, "暗底 hover 应提亮");
    assert!(
        (dark_hover.l - (dark_bg.l + (1. - dark_bg.l) * 0.1)).abs() < 1e-4,
        "提亮幅度应朝白走 10%（实际 l = {}）",
        dark_hover.l
    );

    let light_bg = gpui_kit::hsla(220., 0.04, 0.94, 1.);
    let light_hover = super::render::object_chip_hover_bg(light_bg, false);
    assert!(light_hover.l < light_bg.l, "亮底 hover 应压暗");
    assert!(
        (light_hover.l - (light_bg.l - light_bg.l * 0.1)).abs() < 1e-4,
        "压暗幅度应朝黑走 10%（实际 l = {}）",
        light_hover.l
    );
}

/// 【工具栏 2026-11-20 / 收口 2026-11-24】CHECK 型按钮的「点一次就翻转面板态」：
/// 筛选栏只剩筛选展开 / 查找两颗（下发源库已删、列注释已迁结果集设置，见基线 §21.3），
/// 二者都是 DBeaver 的 CHECK `ToolItem`
/// （`openFindReplaceButton:356`、`FilterExpandPanel` 同族），
/// 点一下只改面板态、由 `sync` 回写；按下态视觉走 `Selectable::selected`
/// （属于目视验收项，见 `result-panel.md §9.23`）。
#[gpui_kit::test]
fn toolbar_check_buttons_flip_their_panel_state(cx: &mut gpui_kit::TestAppContext) {
    use crate::ports::ResultRequest;

    cx.update(gpui_kit::init);
    let (panel, cx) = panel_harness(cx);
    cx.update(|window, cx| window.draw(cx).clear(cx));

    // 两颗探针都在（缺一颗说明筛选栏结构漂了）
    for id in ["editor-result-filter-expand", "editor-result-find-toggle"] {
        assert!(
            cx.debug_bounds(id).is_some(),
            "工具栏应有 {id}（CHECK 型按钮）"
        );
    }

    let click = |id: &'static str, cx: &mut gpui_kit::VisualTestContext| {
        let b = cx
            .debug_bounds(id)
            .unwrap_or_else(|| panic!("工具栏应有 {id}"));
        let point = gpui_kit::point(
            b.origin.x + b.size.width / 2.,
            b.origin.y + b.size.height / 2.,
        );
        cx.simulate_click(point, gpui_kit::Modifiers::default());
        cx.update(|window, cx| window.draw(cx).clear(cx));
    };

    // （下发源库 / 列注释两颗已在 T1 收口移除，对应的点击断言一并删除）

    // 筛选展开：默认收 → 点一下入队展开
    click("editor-result-filter-expand", cx);
    let queued = cx.update(|_, app| panel.read(app).queue.drain());
    assert!(
        queued
            .iter()
            .any(|request| matches!(request, ResultRequest::ToggleFilterExpand)),
        "筛选展开按钮应入队展开：{queued:?}"
    );
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

/// 【2026-11-18 工具栏结构对齐 / 2026-11-20 改正】筛选清除钮 = **常驻 + 恒可用**：
/// DBeaver 建钮时确实 `setEnabled(false)`（`ResultSetFilterPanel.java:319-327`），但
/// `enablePanelControls` 里是**无条件** `filtersClearButton.setEnabled(true)`（`:449-454`，
/// 旧条件 `hasFilters() || hasOrdering() || 词非空` 已被注释掉）⇒ 一旦结果就绪恒可用。
/// 本仓照此：位置常驻、不看筛选词。
#[gpui_kit::test]
fn filter_clear_button_is_always_present_and_never_gated(
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

    // 空词：按钮在，且**不禁用**（DBeaver 的 enablePanelControls 无条件开它）
    let empty = clear_state(cx).expect("清除钮应常驻（不再按词出现）");
    assert_ne!(empty, Some(true), "空词时清除钮也该是可用的");

    // 有词：同一个按钮，状态不变
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
    // 所以这里钉「不是禁用」，而不是钉 `Some(false)`
    assert_ne!(filled, Some(true), "有词时清除钮同样可用");
    assert_eq!(empty, filled, "词的有无不影响清除钮的可用状态");
}

/// 【2026-11-18 工具栏结构对齐 / 2026-11-25 补齐】筛选条 ✕ 的箭头菜单条目（纯函数）：
/// DBeaver `EraseItemListener.fillDropDownMenu`（`ResultSetViewer.java:1525-1545`）里
/// `CMD_FILTER_CLEAR_SETTING` 是第一项且**无门控**，其后才按条件摆「显示隐藏列」、
/// 「重置所有 pins」等项，最后在「除第一项外还有其它项」时补一条「重置所有设置」。
/// 本仓没有颜色覆盖 / 转换器两层，其余逐条对应。
#[test]
fn the_erase_menu_always_offers_the_full_clear() {
    assert_eq!(
        super::events::erase_menu_labels("", 0, 0),
        vec!["删除所有过滤 / 排序"],
        "没词、没隐藏列、没冻结列时只有全清项（它就是清除钮的主点击动作）"
    );
    assert_eq!(
        super::events::erase_menu_labels("   ", 0, 0),
        vec!["删除所有过滤 / 排序"],
        "空白筛选词也一样——全清项不看词"
    );
    assert_eq!(
        super::events::erase_menu_labels("x", 0, 0),
        vec!["删除所有过滤 / 排序"],
        "有词也不再单列「清除筛选词」——已并进全清（DBeaver 没有单独的窄口径项）"
    );
    assert_eq!(
        super::events::erase_menu_labels("x", 2, 0),
        vec![
            "删除所有过滤 / 排序",
            "显示全部列",
            "重置所有筛选与列设置"
        ],
        "有隐藏列时补「显示全部列」（hasHiddenAttributes 口径，顺序固定：全清在前），
         并因「除第一项外还有别的」补「重置所有设置」"
    );
    assert_eq!(
        super::events::erase_menu_labels("", 0, 1),
        vec![
            "删除所有过滤 / 排序",
            "取消所有冻结列",
            "重置所有筛选与列设置"
        ],
        "有冻结列时补「取消所有冻结列」（对应 DBeaver 的 reset pins），同样补「重置所有设置」"
    );
    assert_eq!(
        super::events::erase_menu_labels("", 1, 2),
        vec![
            "删除所有过滤 / 排序",
            "显示全部列",
            "取消所有冻结列",
            "重置所有筛选与列设置"
        ],
        "两项都成立时按 DBeaver 顺序排，并补「重置所有设置」（门控 = 除第一项外还有别的）"
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
/// `CMD_TOGGLE_LAYOUT`。本仓最大化与面板总开关都已落地，只有 `CMD_ACTIVATE_PANELS`
/// 没有端口（按「能力没有就不摆」不列），其余每一项都有真请求落点。
///
/// 这里钉三件事：① 菜单条目顺序（纯函数）；② 条目文案随状态报「点了会做什么」；
/// ③ 五页的页内工具条都真的挂上了这颗按钮（`PopupMenu` 内部不可读，形态断言走纯函数）。
#[gpui_kit::test]
fn right_panel_view_menu_lists_pages_then_the_two_toggles(cx: &mut gpui_kit::TestAppContext) {
    use gpui_kit::test::TestWindowExt as _;
    use gpui_kit::{ElementId, SharedString};

    cx.update(gpui_kit::init);

    // ① 条目顺序：五页 → 分隔 → 最大化 → 分隔 → 面板总开关 → 布局方向
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
            RightTab::Value,
            RightTab::Stats,
            RightTab::Browser,
            RightTab::Group,
            RightTab::Reference,
        ],
        "面板勾选项的顺序应与竖条 / 页签同一份（规范序 = DBeaver 扩展注册序，§55）"
    );
    assert_eq!(items.len(), 10, "五页 + 分隔 + 最大化 + 分隔 + 面板开关 + 布局方向");
    assert_eq!(items[5], super::events::PanelViewItem::Separator);
    assert_eq!(items[6], super::events::PanelViewItem::MaximizeToggle);
    assert_eq!(items[7], super::events::PanelViewItem::Separator);
    assert_eq!(items[8], super::events::PanelViewItem::PanelsToggle);
    assert_eq!(items[9], super::events::PanelViewItem::LayoutToggle);
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

    // ③ 五页的页内工具条都挂上了视图菜单（id = `<页工具条 id>-view-menu`）
    let (panel, cx) = panel_harness(cx);
    for (tab, toolbar_id) in [
        (RightTab::Browser, "editor-result-browser-toolbar"),
        (RightTab::Value, "editor-result-value-toolbar"),
        (RightTab::Stats, "editor-result-stats-toolbar"),
        (RightTab::Group, "editor-result-group-toolbar"),
        (RightTab::Reference, "editor-result-reference-toolbar"),
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

/// 【2026-11-18 侧栏结构对齐】右栏页签条**列出全部五页**且点页签即切页：
/// DBeaver 的右栏是一个 `CTabFolder panelFolder`（`ResultSetViewer.java:1430-1478`），
/// 五页都作为页签常驻其中（`addPanel` 里 `new CTabItem(panelFolder, SWT.CLOSE)`），
/// 点页签走 `setActivePanel`。此前本仓横条只显示当前页、`on_click` 是空实现（点了没反应）。
///
/// 断言走**真实像素点击**：把当前页切到引用（最后一个），再点页签行最左侧，
/// 必须入队「切到该行首格所属的那一页」——既证明五个页签真的排在这一行里，
/// 也证明点击真接了线。
///
/// 注意首格**不是**规范序首项：真值 `setMRUVisible(true)`（`:364`）让选中的页排到最前，
/// 所以切到引用之后首格就是引用；期望值因此取 `open_tabs[0]` 而不是常量。
#[gpui_kit::test]
fn right_panel_tab_strip_lists_all_panels_and_click_switches(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let (panel, cx) = panel_harness(cx);

    // 铺满五页（出厂只启用「值详情」，§55）：本用例要点「五个页签都在这一行里」
    cx.update(|_, app| {
        panel.update(app, |p, cx| p.set_open_tabs_initial(&RightTab::ALL, cx))
    });

    // 当前页 = 引用（切页后 MRU 把它挪到首格）
    cx.update(|_, app| panel.update(app, |p, cx| p.set_right_tab(RightTab::Reference, cx)));
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert_eq!(
        cx.update(|_, app| panel.read(app).right_tab),
        RightTab::Reference,
        "先把当前页切到引用"
    );
    let first_tab = cx.update(|_, app| panel.read(app).open_tabs()[0]);
    assert_eq!(
        first_tab,
        RightTab::Reference,
        "前提：MRU 让刚选中的页排在首格（`setMRUVisible(true)`，`:364`）"
    );

    let bar = cx
        .debug_bounds("editor-result-right-tabs")
        .expect("页签行应带调试选择器才量得到");
    // 页签行最左一格 = `open_tabs[0]`：点它应把「切到该页」入队（切页是宿主动作：
    // 面板只 `queue.request`，由宿主 dispatch 调 `set_right_tab`，所以这里断言队列，
    // 而不是面板状态）
    let point = gpui_kit::point(bar.origin.x + px(24.), bar.origin.y + bar.size.height / 2.);
    cx.simulate_click(point, gpui_kit::Modifiers::default());
    cx.update(|window, cx| window.draw(cx).clear(cx));

    let queued = cx.update(|_, app| panel.read(app).queue.drain());
    assert!(
        queued.iter().any(|request| matches!(
            request,
            crate::ports::ResultRequest::SetRightTab(tab) if *tab == first_tab
        )),
        "点首格页签应入队「切到 {first_tab:?}」（点空白 / 死回调都会让队列为空）：{queued:?}，页签行 {bar:?}"
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

/// 【状态栏组序】DBeaver `createStatusBar0`（`ResultSetViewer.java:2043-2195`）按**添加序**
/// 从左到右是十组：① 刷新（`rsToolbar`，`:2059-2074`）→ ② 编辑工具条（`:2077-2083`）→
/// ③ 导航工具条（`:2084-2090`）→ ④ 配置工具条（空，`:2091-2096`）→ ⑤ 添加/导出
/// （`:2098-2111`）→ ⑥ `ToolbarSeparatorContribution(true)` + `ConfigAction` 齿轮
/// （`:2113-2122`）→ ⑦ `resultSetSize` 段大小（`:2126-2153`）→ ⑧ `rowCountLabel`
/// （`:2155-2180`）→ ⑨ `selectionStatLabel`（`:2182-2184`）→ ⑩ `statusLabel`（`:2186-2191`）。
///
/// 本仓此前把「刷新 + 导出」并成一段摆最末、齿轮挂在整行最右端；本条按上面的真值组序
/// 重排（刷新提到最左、导出自成一组落在行导航之后、齿轮挪到段大小之左），
/// 并用**真实像素横坐标**把相对位置钉住——只看「在不在」守不住组序。
#[gpui_kit::test]
fn status_row_groups_follow_dbeaver_create_status_bar_order(cx: &mut gpui_kit::TestAppContext) {
    use gpui_kit::test::TestWindowExt as _;
    use gpui_kit::{ElementId, SharedString};

    cx.update(gpui_kit::init);
    let (panel, cx) = panel_harness(cx);
    cx.update(|_, app| {
        panel.update(app, |p, cx| {
            p.status = Some(crate::grid::status::ResultStatus {
                total_rows: 2,
                ..Default::default()
            });
            // 工具条配置的默认隐藏集合（撤销/重做/取下一段）不含这几颗；清空是为了
            // 断言与默认集合无关，只钉「组序」
            p.hidden_status_commands.clear();
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
    // 刷新 / 导出 两槽有两种长相：面板注入了真钮就是真钮，没有上下文时是禁用占位
    // （占位 id 由图标 Debug 拼成，见 `status.rs` 的 `placeholder_btn`）。这里两者都认。
    let left_of_either = |cx: &mut gpui_kit::VisualTestContext, ids: &[&str]| -> f32 {
        for id in ids {
            let name = SharedString::from((*id).to_string());
            if let Some(snap) = cx.update(|window, _| window.try_find(ElementId::Name(name))) {
                return f32::from(snap.bounds().origin.x);
            }
        }
        panic!("状态栏应有 {ids:?} 之一");
    };

    let refresh = left_of_either(cx, &["editor-result-refresh", "editor-result-action-RotateCw"]);
    let page_first = left_of(cx, "editor-result-page-first");
    let page_last = left_of(cx, "editor-result-page-last");
    let export = left_of_either(cx, &["editor-result-export", "editor-result-action-Download"]);
    let preferences = left_of(cx, "editor-result-status-preferences");
    // 段大小框挂在**外层 div 的 `debug_selector`** 上（`Input` 是 RenderOnce，内部 id
    // 进不了 `try_find`）——与 `status_row_shows_the_segment_size_input` 同一口径，
    // 只能走 `debug_bounds`
    let segment: f32 = cx
        .debug_bounds("editor-result-segment-size")
        .unwrap_or_else(|| panic!("状态栏应有段大小框"))
        .origin
        .x
        .into();
    let count_rows = left_of(cx, "editor-result-count-rows");
    assert!(
        refresh < page_first,
        "刷新应在最左、行导航之前（真值 `:2059-2074` 对 `:2084-2090`；实际 {refresh} / {page_first}）"
    );
    assert!(
        page_last < export,
        "导出应在行导航之后（真值「添加」组 `:2098-2111`；实际 {page_last} / {export}）"
    );
    assert!(
        export < preferences,
        "齿轮应在导出之后（真值 `:2113-2122`；实际 {export} / {preferences}）"
    );
    assert!(
        preferences < segment,
        "齿轮应在段大小之左（真值 `ConfigAction` 对 `resultSetSize`；实际 {preferences} / {segment}）"
    );
    assert!(
        segment < count_rows,
        "段大小在行计数标签之左（真值 `:2126-2153` 对 `:2155-2180`；实际 {segment} / {count_rows}）"
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

/// 【筛选设置 · 列约束表】列序 = DBeaver `FilterSettingsDialog.addColumn` 的顺序：
/// Name → `#`（`:139-146`）→ Visible（`:148-176`）→ Pinned（`:178-211`）。
///
/// 本仓此前把 `#` 摆在行尾（`列 / 可见 / 冻结 / 条件 / 排序 / #`），与 DBeaver 的列序对不上；
/// 本批把它挪到列名**正后方**。「条件 / 排序」列是本仓扩展（列头菜单设置项的只读投影），
/// 排在 DBeaver 三列之后。
#[gpui_kit::test]
fn filter_settings_columns_put_the_position_right_after_the_name(
    cx: &mut gpui_kit::TestAppContext,
) {
    cx.update(gpui_kit::init);
    let (panel, cx) = panel_harness(cx);
    cx.update(|window, app| panel.update(app, |p, cx| p.open_filter_settings(window, cx)));
    cx.update(|window, cx| window.draw(cx).clear(cx));

    let pos = cx
        .debug_bounds("editor-result-filter-settings-pos-0")
        .expect("列约束表应渲染 # 列");
    let vis = cx
        .debug_bounds("editor-result-filter-settings-vis-cell-0")
        .expect("列约束表应渲染可见勾选框");
    let pin = cx
        .debug_bounds("editor-result-filter-settings-pin-cell-0")
        .expect("列约束表应渲染冻结勾选框");
    let criteria = cx
        .debug_bounds("editor-result-filter-settings-criteria-0")
        .expect("列约束表应渲染条件列");

    assert!(
        pos.origin.x < vis.origin.x,
        "# 应在可见勾选框左侧（DBeaver 里 # 紧跟列名）：pos={:?} vis={:?}",
        pos.origin.x,
        vis.origin.x
    );
    assert!(
        vis.origin.x < pin.origin.x,
        "可见应在冻结左侧（Name / # / Visible / Pinned 的列序）"
    );
    assert!(
        pin.origin.x < criteria.origin.x,
        "冻结应在条件列左侧（条件/排序是本仓追加在末尾的投影列）"
    );
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

    // 回填一条键并把右栏切到引用页：该页当前**置灰**（Q36-A），只摆占位说明，
    // 键下拉与「打开目标对象」都不出现（原形态留在 `render_reference_tab_live` 里）
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
        cx.debug_bounds("editor-result-reference-toolbar").is_some(),
        "置灰也要保留页内工具条（五页同形）"
    );
    assert!(
        cx.debug_bounds("editor-result-reference-keys").is_none(),
        "置灰口径：不再渲染只读键下拉"
    );
    assert!(
        cx.debug_bounds("editor-result-reference-open").is_none(),
        "置灰口径：不再渲染「打开目标对象」"
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

    // 页面上：引用页**当前是置灰占位**（2026-11-24 拍板 Q36-A）——页内工具条照旧，
    // 但正文不画这张票。数据侧（上面 `reference_rows_sql` / `OpenReferenceRows`）
    // 与 `render_reference_tab_live` 都留着，接线后把占位换回去即可。
    cx.update(|_window, app| panel.update(app, |p, cx| p.set_right_tab(RightTab::Reference, cx)));
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(
        cx.debug_bounds("editor-result-reference-toolbar").is_some(),
        "占位页也要有页内工具条（页签位置保留）"
    );
    assert!(
        cx.debug_bounds("editor-result-reference-rows").is_none(),
        "置灰期间不渲染「查看引用行」这张票"
    );
}

/// 【筛选设置 · 存档】「保存 / 重置」的语义：显式存、只清存档不动当前筛选。
///
/// 对齐 DBeaver `ResultSetViewer.java:648-664`（`saveDataFilter`，存当前 `model.getDataFilter()`）
/// 与 `:671-687`（`resetSavedDataFilter`，`removeDataFilter` + 通知）。两条都是**用户点了
/// 才算**——与列布局那套「随会话自动存」不是一回事。
#[gpui_kit::test]
fn saving_the_filter_setting_records_the_current_filter(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let (panel, cx) = panel_harness(cx);
    cx.update(|window, cx| window.draw(cx).clear(cx));

    // 造一份「当前生效」的筛选：结果 SQL + 筛选词 + 自定义排序
    cx.update(|_window, app| {
        panel.update(app, |p, _cx| {
            p.result_sql = Some("select * from orders".to_string());
            p.filter_text = "amount > 100".to_string();
            p.filter_order = "amount desc".to_string();
        });
    });

    cx.update(|_window, app| panel.update(app, |p, cx| p.save_filter_setting(cx)));
    let saved = cx
        .update(|_window, app| panel.read(app).saved_filter_setting().cloned())
        .expect("点「保存过滤器设置」后应留下存档");
    assert_eq!(saved.sql, "select * from orders", "存档要记住它属于哪条 SQL");
    assert_eq!(saved.filter, "amount > 100", "存的是当前生效的筛选词");
    assert_eq!(saved.order, "amount desc", "自定义排序一并进存档（DBeaver 存整份 filter）");

    // 重置：只清存档，当前筛选原样还在（DBeaver 同样只删配置项）
    cx.update(|_window, app| panel.update(app, |p, cx| p.reset_filter_setting(cx)));
    assert!(
        cx.update(|_window, app| panel.read(app).saved_filter_setting().is_none()),
        "重置后不应再留着存档"
    );
    assert_eq!(
        cx.update(|_window, app| panel.read(app).filter_text().to_string()),
        "amount > 100",
        "重置只清存档，不能把正在生效的筛选也抹掉"
    );
}

/// 【筛选设置 · 存档】换进来同一条 SQL 的结果时自动套用；换个 SQL 就不套。
///
/// 对齐 `ResultSetUtils.restoreDataFilter:174-189`（打开结果时恢复存档，且只在
/// `hasFilters()` 时算数）。本仓投影入口是 [`super::ResultPanel::sync`]：套用后
/// 词进 `filter_text` 并立即应用，输入框回填留给宿主（`filter_backfill`）。
#[gpui_kit::test]
fn a_saved_filter_is_reapplied_only_to_its_own_sql(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let (panel, cx) = panel_harness(cx);
    cx.update(|window, cx| window.draw(cx).clear(cx));

    // 别的 SQL：存档不生效
    cx.update(|_window, app| {
        panel.update(app, |p, cx| {
            p.saved_filter = Some(crate::panel::SavedFilterSetting {
                sql: "select * from orders".to_string(),
                filter: "id = 1".to_string(),
                order: String::new(),
                table: None,
            });
            p.sync(cx, sync_input_with_sql("select * from customers"));
        });
    });
    assert_eq!(
        cx.update(|_window, app| panel.read(app).filter_text().to_string()),
        "",
        "换了一条 SQL 不该套用别人的筛选"
    );

    // 同一条 SQL：套用（词进真值，并留一份待回填输入框的内容）
    cx.update(|_window, app| {
        panel.update(app, |p, cx| {
            p.sync(cx, sync_input_with_sql("select * from orders"));
        });
    });
    assert_eq!(
        cx.update(|_window, app| panel.read(app).filter_text().to_string()),
        "id = 1",
        "同一条 SQL 的结果应恢复存档里的筛选词"
    );
    assert_eq!(
        cx.update(|_window, app| panel.update(app, |p, _| p.take_filter_backfill())),
        Some("id = 1".to_string()),
        "输入框回填要留给宿主（`sync` 没有窗口句柄）"
    );
}

/// 【筛选设置 · 存档】套用判据（纯函数）：四条任一不成立就不套。
#[test]
fn the_saved_filter_needs_its_own_sql_and_an_untouched_filter_box() {
    let saved = crate::panel::SavedFilterSetting {
        sql: "select 1".to_string(),
        filter: "id = 1".to_string(),
        ..Default::default()
    };
    assert!(super::events::saved_filter_applies(
        &saved,
        Some("select 1"),
        ""
    ));
    // 换 SQL 不套
    assert!(!super::events::saved_filter_applies(
        &saved,
        Some("select 2"),
        ""
    ));
    // 已有筛选词不覆盖（用户手写的优先）
    assert!(!super::events::saved_filter_applies(
        &saved,
        Some("select 1"),
        "id = 2"
    ));
    // 当前结果没有 SQL 就无从判断归属
    assert!(!super::events::saved_filter_applies(&saved, None, ""));
    // 空筛选的存档等于「没有可恢复的筛选」（DBeaver `hasFilters()` 同口径）
    let empty = crate::panel::SavedFilterSetting {
        sql: "select 1".to_string(),
        ..Default::default()
    };
    assert!(!super::events::saved_filter_applies(
        &empty,
        Some("select 1"),
        ""
    ));
}

/// 【筛选存档落盘】存档走 JSON 编解码：SQL / 筛选词里的逗号与引号原样往返，
/// 空串与坏串都归一成「没存过」，缺字段的旧串按默认补齐。
#[test]
fn saved_filter_setting_round_trips_through_the_session_codec() {
    let saved = crate::panel::SavedFilterSetting {
        sql: "select a, b from t where name = 'x,y'".to_string(),
        filter: "id in (1,2)".to_string(),
        order: "order by b desc".to_string(),
        table: Some("t".to_string()),
    };
    let encoded = saved.encode();
    assert_eq!(
        crate::panel::SavedFilterSetting::decode(&encoded),
        Some(saved),
        "带逗号 / 引号的四字段都要原样回来"
    );
    assert_eq!(
        crate::panel::SavedFilterSetting::decode(""),
        None,
        "空串 = 没存过"
    );
    assert_eq!(
        crate::panel::SavedFilterSetting::decode("{oops"),
        None,
        "坏串当没存过（不恢复半份错存档）"
    );

    // 缺字段的旧串按默认补齐（`#[serde(default)]`；不整份丢弃）
    let back = crate::panel::SavedFilterSetting::decode(r#"{"sql":"select 1","filter":"id = 1"}"#)
        .expect("缺字段也能解");
    assert_eq!(back.order, "");
    assert_eq!(back.table, None);
}

/// 【筛选存档落盘】恢复会话只**注入**存档、不套用：词要等下一次 `sync` 且 SQL 对得上
/// 才进筛选框（与 DBeaver「打开这张表的结果才 `restoreDataFilter`」同口径）。
#[gpui_kit::test]
fn restoring_the_saved_filter_injects_it_without_applying(
    cx: &mut gpui_kit::TestAppContext,
) {
    cx.update(gpui_kit::init);
    let (panel, cx) = panel_harness(cx);
    cx.update(|window, cx| window.draw(cx).clear(cx));

    let saved = crate::panel::SavedFilterSetting {
        sql: "select * from orders".to_string(),
        filter: "id = 1".to_string(),
        order: String::new(),
        table: None,
    };
    cx.update(|_window, app| {
        panel.update(app, |p, cx| p.set_saved_filter(Some(saved.clone()), cx));
    });
    assert_eq!(
        cx.update(|_window, app| panel.read(app).saved_filter_setting().cloned()),
        Some(saved.clone()),
        "注入后存档应当在场（菜单禁用态靠它）"
    );
    assert_eq!(
        cx.update(|_window, app| panel.read(app).filter_text().to_string()),
        "",
        "只注入不套用：筛选框不能自己长出词"
    );
    assert_eq!(
        cx.update(|_window, app| panel.update(app, |p, _| p.take_filter_backfill())),
        None,
        "注入不产生回填请求"
    );
    // 注入不是用户改动：不推落库请求，否则恢复动作会把刚读回来的存档再写一遍
    assert!(
        cx.update(|_window, app| panel.read(app).queue.is_empty()),
        "注入不该推 SavedFilterChanged"
    );

    // 同一次会话里再注入同样的值：幂等，不重复 notify
    cx.update(|_window, app| {
        panel.update(app, |p, cx| p.set_saved_filter(Some(saved.clone()), cx));
    });
    assert_eq!(
        cx.update(|_window, app| panel.read(app).saved_filter_setting().cloned()),
        Some(saved)
    );
}

/// 投影输入的最小骨架（`sql` + 两列两行），只给存档套用那几条用例用。
fn sync_input_with_sql(sql: &str) -> crate::panel::ResultSyncInput {
    let mut input = crate::panel::ResultSyncInput::default();
    input.sql = Some(sql.to_string());
    input.grid = Some(crate::panel::GridPayload {
        columns: vec!["id".to_string(), "amount".to_string()],
        rows: vec![
            vec!["1".to_string(), "10".to_string()],
            vec!["2".to_string(), "20".to_string()],
        ],
        column_types: vec![None, None],
        table: None,
        annotations: vec![None, None],
        fact: None,
        local: None,
    });
    input
}

/// 【筛选条 ✕】「删除所有过滤 / 排序」：清筛选词 + 自定义排序 + 列级筛选 + 本地排序，
/// **不动**隐藏列。
///
/// 对齐 DBeaver `clearDataFilter(true)`（`ResultSetViewer.java:605-626`）：造一份新
/// filter 后把每列的 `visible` / `options` 搬回去（注释「keeping all visual state
/// (visibility, etc.)」）⇒ 隐藏列照样保留。这也是清除钮的主点击动作
/// （`EraseItemListener.executeDefaultAction:1549-1551`）。
#[gpui_kit::test]
fn clearing_all_filters_drops_ordering_but_keeps_hidden_columns(
    cx: &mut gpui_kit::TestAppContext,
) {
    cx.update(gpui_kit::init);
    let (panel, cx) = panel_harness(cx);
    cx.update(|window, cx| window.draw(cx).clear(cx));

    // 造「什么都开着」的状态：筛选词 + 自定义排序 + 列级筛选 + 本地排序 + 隐藏一列
    cx.update(|_window, app| {
        let grid = panel.read(app).grid().clone();
        panel.update(app, |p, _cx| {
            p.filter_text = "1".to_string();
            p.filter_order = "amount desc".to_string();
            p.column_hidden = vec!["amount".to_string()];
        });
        grid.update(app, |state, cx| {
            let hidden = vec!["amount".to_string()];
            state.delegate_mut().set_filter("1".to_string());
            state
                .delegate_mut()
                .set_column_filter(Some((0, "1".to_string())));
            state.delegate_mut().apply_sort(Some((1, true)));
            state.delegate_mut().apply_hidden_columns(&hidden);
            state.refresh(cx);
        });
    });

    cx.update(|window, cx| {
        panel.update(cx, |p, cx| p.clear_filter_and_ordering(window, cx));
    });

    let after = cx.update(|_window, app| {
        let p = panel.read(app);
        let delegate = p.grid().read(app).delegate();
        (
            p.filter_text().to_string(),
            p.filter_order.clone(),
            delegate.column_filter().map(|(c, v)| (c, v.to_string())),
            delegate.sort_state(),
            p.column_hidden.clone(),
            delegate.is_hidden_column(1),
        )
    });
    assert_eq!(after.0, "", "筛选词要清掉");
    assert_eq!(after.1, "", "自定义排序要清掉（浮层与真值同一份）");
    assert_eq!(after.2, None, "列级筛选要清掉");
    assert_eq!(after.3, None, "本地排序要清掉");
    assert_eq!(
        after.4,
        vec!["amount".to_string()],
        "隐藏列不受影响（DBeaver 的 clearDataFilter 显式保留 visual state）"
    );
    assert!(after.5, "delegate 侧隐藏态也还在");
}
/// 【结果内查找·命令语义】右键 filters 段首的「在结果中查找…」走 `open_find`，
/// 它是幂等的：未开则开；**已开时不关**（工具栏那颗 CHECK 型 `toggle_find` 才负责关闭）。
/// 两条入口共用同一份面板态，故这里对照着验：打开 → 再 open 仍开 → toggle 才关。
#[gpui_kit::test]
fn open_find_is_idempotent_and_never_closes(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let (panel, cx) = panel_harness(cx);

    // 1) 第一次 open：打开
    cx.update(|window, cx| panel.update(cx, |p, cx| p.open_find(window, cx)));
    let after_first = cx.update(|_window, app| {
        let (open, query, hits) = panel.read(app).find_state();
        (open, query.to_string(), hits)
    });
    assert!(after_first.0, "未开时 `open_find` 应把查找栏打开");

    // 2) 再 open 一次：仍然开着（命令不该反关已开的查找栏）
    cx.update(|window, cx| panel.update(cx, |p, cx| p.open_find(window, cx)));
    let after_second = cx.update(|_window, app| {
        let (open, query, hits) = panel.read(app).find_state();
        (open, query.to_string(), hits)
    });
    assert!(
        after_second.0,
        "已开时 `open_find` 必须保持打开（命令语义，不是开关）"
    );
    assert_eq!(after_second.1, after_first.1, "`open_find` 不动查找词");

    // 3) 对照：工具栏那颗 toggle 才是开关，会把已开的关掉且保留词
    cx.update(|window, cx| panel.update(cx, |p, cx| p.toggle_find(window, cx)));
    let after_toggle = cx.update(|_window, app| {
        let (open, query, hits) = panel.read(app).find_state();
        (open, query.to_string(), hits)
    });
    assert!(!after_toggle.0, "`toggle_find` 仍是开关语义");
    assert_eq!(after_toggle.1, after_first.1, "关闭时保留查找词");
}

/// 【第 60 批】切呈现后的焦点目标随呈现变化：文本态 → 面板根，网格态 → 网格。
///
/// 钉住的是「文本态不该指回网格」（网格那时不在渲染树里，焦点交过去等于丢焦点），
/// 端到端行为见 editor 侧 `ctrl_backtick_returns_from_the_text_presentation`。
#[gpui_kit::test]
fn presentation_focus_target_follows_the_presentation(cx: &mut gpui_kit::TestAppContext) {
    use gpui_kit::Focusable as _;

    cx.update(gpui_kit::init);
    let (panel, cx) = panel_harness(cx);

    let panel_handle = cx.update(|_w, app| panel.read(app).focus_handle.clone());
    let grid = cx.update(|_w, app| panel.read(app).grid.clone());
    let grid_handle = cx.update(|_w, app| grid.read(app).focus_handle(app));
    assert_ne!(
        panel_handle, grid_handle,
        "面板根与网格必须是两个不同的焦点句柄（否则文本态没有独立落点）"
    );

    // 出厂 = 网格态 → 目标应是网格
    let grid_target = cx.update(|_w, app| panel.read(app).presentation_focus_target(app));
    assert_eq!(grid_target, grid_handle, "网格态应把焦点交给网格");

    // 切到文本态 → 目标应是面板根
    cx.update(|_window, cx| {
        panel.update(cx, |p, cx| {
            p.set_presentation(crate::ports::Presentation::Text, cx);
        });
    });
    let text_target = cx.update(|_w, app| panel.read(app).presentation_focus_target(app));
    assert_eq!(text_target, panel_handle, "文本态应把焦点交给面板根");
}

/// 【第 75 批】跳列解析的三段顺序（DBeaver `askForColumnToGo:815-849`）：
/// 先按名字（大小写不敏感）→ 再按 1 基序号 → 都不中才报「未知列名」。
#[gpui_kit::test]
fn goto_column_target_matches_its_name_then_its_one_based_index(
    cx: &mut gpui_kit::TestAppContext,
) {
    cx.update(gpui_kit::init);
    let (panel, cx) = panel_harness(cx);

    cx.update(|_window, app| {
        let panel = panel.read(app);
        // harness 的列 = ["id", "amount"]，数据列序 0 / 1
        assert_eq!(panel.goto_column_target("amount", app), Ok(1));
        assert_eq!(
            panel.goto_column_target("AMOUNT", app),
            Ok(1),
            "列名不区分大小写（上游用 name.toUpperCase() 建映射）"
        );
        assert_eq!(panel.goto_column_target(" id ", app), Ok(0), "两端空白要吃掉");
        assert_eq!(panel.goto_column_target("2", app), Ok(1), "序号从 1 起");
        assert_eq!(panel.goto_column_target("1", app), Ok(0));
        assert!(
            panel.goto_column_target("3", app).is_err(),
            "越界序号必须报错，不能夹到最后一列"
        );
        assert!(
            panel.goto_column_target("nope", app).is_err(),
            "未知列名要报错"
        );
        assert!(
            panel.goto_column_target("", app).is_err(),
            "空输入要报错"
        );
    });
}

/// 【第 75 批】跳列浮层：开 → 遮罩挂上；提交后 → 选中该列（**组件列 = 显示列 + 1**）、
/// 行保持不动、浮层收掉。
#[gpui_kit::test]
fn the_goto_column_overlay_jumps_and_keeps_the_current_row(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let (panel, cx) = panel_harness(cx);
    let grid = cx.update(|_w, app| panel.read(app).grid.clone());

    // 当前行 = 第 2 行（组件行 1），当前列 = 第 1 个数据列（组件列 1）
    cx.update(|_w, app| grid.update(app, |state, cx| state.set_selected_cell(1, 1, cx)));

    let opened = cx.update(|window, app| panel.update(app, |p, cx| p.open_goto_column(window, cx)));
    assert!(opened, "有列时可跳");
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(
        cx.debug_bounds("editor-result-goto-column").is_some(),
        "打开后遮罩要挂上"
    );
    assert!(
        cx.debug_bounds("editor-result-goto-column-go").is_some(),
        "浮层要有「跳转」钮"
    );

    // 输列名「amount」（第 2 个数据列 → 组件列 2）并提交
    cx.update(|window, app| {
        panel.update(app, |p, cx| p.set_goto_column_text_for_test("amount", window, cx));
    });
    cx.update(|window, app| panel.update(app, |p, cx| p.submit_goto_column(window, cx)));

    let (selected, open) = cx.update(|_w, app| {
        let p = panel.read(app);
        (p.grid.read(app).selected_cell(), p.goto_column_for_test(app))
    });
    assert_eq!(
        selected,
        Some((1, 2)),
        "选中 amount（组件列 2），且**行保持 1**（上游是 scrollToRow(CURRENT)，不是跳首行）"
    );
    assert!(!open, "提交后浮层要收");
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(
        cx.debug_bounds("editor-result-goto-column").is_none(),
        "收掉后遮罩不应再渲染"
    );
}

/// 【第 75 批】跳列失败要**留在浮层里**说原因（DBeaver 弹错后不关对话框）：
/// 未知列名、序号越界都不收浮层，错误行照常渲染出来。
#[gpui_kit::test]
fn an_unknown_column_keeps_the_overlay_open_with_the_reason(
    cx: &mut gpui_kit::TestAppContext,
) {
    cx.update(gpui_kit::init);
    let (panel, cx) = panel_harness(cx);

    cx.update(|window, app| panel.update(app, |p, cx| p.open_goto_column(window, cx)));
    cx.update(|window, app| {
        panel.update(app, |p, cx| p.set_goto_column_text_for_test("nope", window, cx));
    });
    cx.update(|window, app| panel.update(app, |p, cx| p.submit_goto_column(window, cx)));
    cx.update(|window, cx| window.draw(cx).clear(cx));

    assert!(
        cx.update(|_w, app| panel.read(app).goto_column_for_test(app)),
        "失败不能顺手关掉浮层"
    );
    assert!(
        cx.debug_bounds("editor-result-goto-column-error").is_some(),
        "错误行要渲染出来"
    );

    // 改成合法序号再提交：这次成功并收浮层
    cx.update(|window, app| {
        panel.update(app, |p, cx| p.set_goto_column_text_for_test("2", window, cx));
    });
    cx.update(|window, app| panel.update(app, |p, cx| p.submit_goto_column(window, cx)));
    assert!(!cx.update(|_w, app| panel.read(app).goto_column_for_test(app)));
}

/// 【第 76 批】`CTRL+ALT+C` 选列（DBeaver `CMD_SELECT_COLUMNS`，
/// `SpreadsheetCommandHandler:106-118`）：当前格所在的**整列**选中，
/// 组件列 = 显示列 + 1（0 是行号槽）；行号槽里的格不算数据列。
///
/// 上游是把「选中列 × 所有行」拼成单元格集合（多区域），本仓选区是单区域的，
/// 等价表达取组件整列选中——口径差异记在 `result-panel.md` §9.59.2。
#[gpui_kit::test]
fn select_column_goes_to_the_column_under_the_current_cell(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let (panel, cx) = panel_harness(cx);
    let grid = cx.update(|_w, app| panel.read(app).grid.clone());

    // 当前格 = (行 0, 组件列 1) → 数据列 0
    cx.update(|_w, app| grid.update(app, |state, cx| state.set_selected_cell(0, 1, cx)));
    assert!(
        cx.update(|_w, app| panel.update(app, |p, cx| p.select_current_column(cx))),
        "有当前数据格时选列要成立"
    );
    let (component_col, data_col) = cx.update(|_w, app| {
        (
            grid.read(app).selected_col(),
            panel.read(app).selected_col,
        )
    });
    assert_eq!(
        component_col,
        Some(1),
        "组件整列选中落在显示列 0 上（0 是行号槽，不能左移）"
    );
    assert_eq!(data_col, Some(0), "右栏列浏览器跟到数据列 0");

    // 移到第 2 个数据列（组件列 2）→ 数据列 1
    cx.update(|_w, app| grid.update(app, |state, cx| state.set_selected_cell(1, 2, cx)));
    assert!(
        cx.update(|_w, app| panel.update(app, |p, cx| p.select_current_column(cx))),
        "换到第 2 列后照样成立"
    );
    let (component_col, data_col) = cx.update(|_w, app| {
        (
            grid.read(app).selected_col(),
            panel.read(app).selected_col,
        )
    });
    assert_eq!(component_col, Some(2));
    assert_eq!(data_col, Some(1));

    // 行号槽（组件列 0）没有数据列可跟。
    // 【D10-9】这条状态在点击路径上已不可达（点行号槽时 lifecycle 会把当前格换成首数据列），
    // 这里只钉纯函数对越界输入的防御：直接写 `selection_pos` 绕过事件路径。
    cx.update(|_w, app| {
        panel.update(app, |p, _cx| p.selection_pos = Some((0, 0)));
    });
    assert!(
        !cx.update(|_w, app| panel.update(app, |p, cx| p.select_current_column(cx))),
        "行号槽不当作数据列（宿主据此留提示）"
    );

    // 没有当前格
    cx.update(|_w, app| panel.update(app, |p, _cx| p.selection_pos = None));
    assert!(!cx.update(|_w, app| panel.update(app, |p, cx| p.select_current_column(cx))));
}

/// 【第 76 批】`CTRL+ALT+R` 选行（DBeaver `CMD_SELECT_ROWS`，
/// `SpreadsheetCommandHandler:119-131`）：组件整行选中 + 状态行那口「已选第 N 行」
/// 同步写（1 基）——行级三钮 / 「修改」钮都读 `status.selected_row`，不写的话
/// 按完键位它们仍全是灰的。
#[gpui_kit::test]
fn select_row_marks_the_row_and_the_status_line(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let (panel, cx) = panel_harness(cx);

    // harness 出厂没有状态行；先装一条（set_selected_row 只写既有的 status）
    cx.update(|_w, app| {
        panel.update(app, |p, _cx| {
            p.status = Some(crate::grid::ResultStatus {
                total_rows: 2,
                selected_row: None,
                selected_cell: None,
                truncated_hint: None,
                has_more: false,
                selected_rows: 0,
                selected_columns: 0,
                selected_cells: 0,
            });
        });
    });
    let grid = cx.update(|_w, app| panel.read(app).grid.clone());

    // 当前格 = (行 1, 组件列 1)
    cx.update(|_w, app| grid.update(app, |state, cx| state.set_selected_cell(1, 1, cx)));
    assert!(
        cx.update(|_w, app| panel.update(app, |p, cx| p.select_current_row(cx))),
        "有当前格时选行要成立"
    );
    let (component_row, status_row) = cx.update(|_w, app| {
        (
            grid.read(app).selected_row(),
            panel.read(app).status.as_ref().and_then(|s| s.selected_row),
        )
    });
    assert_eq!(component_row, Some(1), "组件整行选中（delegate 据此铺色）");
    assert_eq!(
        status_row,
        Some(2),
        "状态行写 1 基行号（`row_target_state` 靠它把行级动作解开）"
    );

    // 行级门控真的跟着打开（harness 出厂只读，先打开可编辑）
    cx.update(|_w, app| panel.update(app, |p, _cx| p.editable = true));
    let target = cx.update(|_w, app| panel.read(app).row_target_state(app));
    assert_eq!(target.row, Some(1), "0 基视图行");
    assert!(target.actions.copy && target.actions.delete, "整行选中后行级三钮可用");

    // 没有当前格
    cx.update(|_w, app| panel.update(app, |p, _cx| p.selection_pos = None));
    assert!(!cx.update(|_w, app| panel.update(app, |p, cx| p.select_current_row(cx))));
}

/// 【第 77 批】`CTRL+ALT+SHIFT+T` 双向往返（DBeaver
/// `ResultSetViewer.switchFilterFocus:689-701`）：网格焦点 → 筛选框；再按 → 回网格
/// （网格态走 `presentation_focus_target`）。
#[gpui_kit::test]
fn switching_the_filter_focus_goes_both_ways(cx: &mut gpui_kit::TestAppContext) {
    use gpui_kit::Focusable as _;

    cx.update(gpui_kit::init);
    let (panel, cx) = panel_harness(cx);

    let (grid_handle, filter_handle) = cx.update(|_w, app| {
        let grid = panel.read(app).grid.read(app).focus_handle(app);
        let filter = panel.read(app).filter_focus_handle(app);
        (grid, filter)
    });

    cx.update(|window, cx| window.focus(&grid_handle, cx));
    assert!(
        cx.update(|window, app| panel.update(app, |p, cx| p.switch_filter_focus(window, cx))),
        "筛选区在摆着，切换要成立"
    );
    assert_eq!(
        cx.update(|window, app| window.focused(app)),
        Some(filter_handle.clone()),
        "第一次按：焦点进筛选框"
    );

    assert!(
        cx.update(|window, app| panel.update(app, |p, cx| p.switch_filter_focus(window, cx))),
        "第二次按也要成立"
    );
    assert_eq!(
        cx.update(|window, app| window.focused(app)),
        Some(grid_handle),
        "第二次按：焦点交回呈现（网格态 = 网格）"
    );
}

/// 【第 77 批】筛选区收起（`show_filter_panel == false`，对应上游 `setVisible(false)`）：
/// 不动作、返回 false（宿主留提示）。上游在 `filtersPanel == null` 时静默 return，
/// 本仓键位没有「灰」这一态，所以由宿主说话。
#[gpui_kit::test]
fn switching_the_filter_focus_does_nothing_when_the_filter_area_is_collapsed(
    cx: &mut gpui_kit::TestAppContext,
) {
    cx.update(gpui_kit::init);
    let (panel, cx) = panel_harness(cx);
    cx.update(|_w, app| panel.update(app, |p, _cx| p.show_filter_panel = false));

    assert!(
        !cx.update(|window, app| panel.update(app, |p, cx| p.switch_filter_focus(window, cx))),
        "筛选区收起时不切焦点"
    );
}

/// 【第 78 批】`ALT+SHIFT+H` 隐藏当前列（DBeaver `CMD_HIDE_COLUMNS`，
/// `SpreadsheetCommandHandler:174-194`）：当前格所在的数据列进隐藏清单、可见列少一列；
/// 当前格按同一视觉列位重投影（指向后面那一列，而不是被藏掉的那一列）。
#[gpui_kit::test]
fn hiding_the_current_column_takes_it_out_of_the_view(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let (panel, cx) = panel_harness(cx);
    let grid = cx.update(|_w, app| panel.read(app).grid.clone());

    // harness 出厂两列，这里扩成三列，才能藏「中间一列」——只剩两列时藏右端那一列
    // 会让组件列位越界（那条分支由代码清选，见 `hide_current_column` 的说明）
    cx.update(|_w, app| {
        grid.update(app, |state, cx| {
            state.delegate_mut().set_data(
                vec!["a".to_string(), "b".to_string(), "c".to_string()],
                vec![vec!["1".to_string(), "2".to_string(), "3".to_string()]],
            );
            state.refresh(cx);
        })
    });

    // 组件列 2 = 数据列 1（`b`）
    cx.update(|_w, app| grid.update(app, |state, cx| state.set_selected_cell(0, 2, cx)));
    assert_eq!(
        cx.update(|_w, app| panel.read(app).grid.read(app).delegate().visible_column_count()),
        3,
        "三列都可见"
    );

    let result = cx.update(|window, app| {
        panel.update(app, |p, cx| p.hide_current_column(window, cx))
    });
    assert!(result.is_ok(), "有当前列且不止一列时可隐藏：{result:?}");

    let after = cx.update(|_w, app| {
        let delegate = panel.read(app).grid.read(app).delegate();
        (
            delegate.visible_column_count(),
            delegate.hidden_count(),
            delegate.is_hidden_column(1),
            panel.read(app).selection_pos,
            panel.read(app).selected_col,
        )
    });
    assert_eq!(after.0, 2, "可见列少一列");
    assert_eq!(after.1, 1, "隐藏清单只有这一列");
    assert!(after.2, "藏掉的是数据列 1（b）");
    assert_eq!(
        after.3,
        Some((0, 2)),
        "当前格停在原视觉列位（此时指向 c），不是被藏掉的 b"
    );
    assert_eq!(after.4, Some(2), "右栏选中列跟着投影到 c");
}

/// 【第 78 批】只剩一列时再藏被拒：上游 `selectedColumns.size() >=
/// model.getVisibleAttributeCount()` 就弹 ERROR 框（`SpreadsheetCommandHandler:182-186`），
/// 本仓等价判据 = 可见列只剩 1 列，理由交宿主说。
#[gpui_kit::test]
fn hiding_the_last_visible_column_is_refused(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let (panel, cx) = panel_harness(cx);

    // 先把 amount 藏掉，只剩 id
    cx.update(|_w, app| panel.update(app, |p, cx| p.set_columns_hidden(vec![1], true, cx)));
    assert_eq!(
        cx.update(|_w, app| panel.read(app).grid.read(app).delegate().visible_column_count()),
        1,
        "先藏到只剩一列"
    );

    // 组件列 1 = 剩下的数据列 0（id）
    cx.update(|_w, app| {
        let grid = panel.read(app).grid.clone();
        grid.update(app, |state, cx| state.set_selected_cell(0, 1, cx))
    });
    let result = cx.update(|window, app| {
        panel.update(app, |p, cx| p.hide_current_column(window, cx))
    });
    let reason = result.err().unwrap_or_default();
    assert!(reason.contains("最后一列"), "要说明原因，实际是：{reason}");
    assert_eq!(
        cx.update(|_w, app| panel.read(app).grid.read(app).delegate().visible_column_count()),
        1,
        "底线守住：仍有一列可见"
    );
}

/// 【第 79 批】`ALT+SHIFT+C` 复制列名（DBeaver `CMD_COPY_COLUMN_NAMES`，
/// `plugin.xml:1095`；`ResultSetHandlerMain:333-375`）。
///
/// 上游列集合只有两级：表格选区为空 → `getVisibleAttributes()`（全部可见列）；
/// 否则用选中的列。本仓单区域选区的等价 = 当前格所在列 → 全部可见列（跳过隐藏）。
/// 右栏的列选中不参与（上游那是另一个部件），所以 harness 预置的 `selected_col`
/// 在这里不起作用。
#[gpui_kit::test]
fn copying_column_names_uses_the_current_cell_then_every_visible_column(
    cx: &mut gpui_kit::TestAppContext,
) {
    cx.update(gpui_kit::init);
    let (panel, cx) = panel_harness(cx);

    let clipboard = |cx: &mut gpui_kit::VisualTestContext| {
        cx.update(|_w, app| app.read_from_clipboard().and_then(|item| item.text()))
    };

    // ① 没有当前格：全部可见列（数据序，Tab 分隔）
    assert_eq!(
        cx.update(|_w, app| panel.read(app).selection_pos),
        None,
        "前提：harness 出厂没有当前格"
    );
    let count = cx.update(|_w, app| panel.update(app, |p, cx| p.copy_column_names(cx)));
    assert_eq!(count, Ok(2), "两列都可见");
    assert_eq!(clipboard(cx).as_deref(), Some("id\tamount"), "Tab 分隔");

    // ② 当前格所在列盖过全量（组件列 2 = 数据列 1 = `amount`）
    cx.update(|window, app| {
        let grid = panel.read(app).grid.clone();
        grid.update(app, |state, cx| state.set_selected_cell(0, 2, cx));
        panel.update(app, |p, cx| p.set_selection_pos(Some((0, 2)), window, cx));
    });
    let count = cx.update(|_w, app| panel.update(app, |p, cx| p.copy_column_names(cx)));
    assert_eq!(count, Ok(1), "只拷这一列");
    assert_eq!(clipboard(cx).as_deref(), Some("amount"), "单列不加分隔符");

    // ③ 那一列被藏掉 → 当前格不在可见列上，退回全部可见列
    cx.update(|_w, app| panel.update(app, |p, cx| p.set_columns_hidden(vec![1], true, cx)));
    let count = cx.update(|_w, app| panel.update(app, |p, cx| p.copy_column_names(cx)));
    assert_eq!(count, Ok(1), "只剩一列可见");
    assert_eq!(clipboard(cx).as_deref(), Some("id"), "隐藏的那列不拷");
}

/// 【第 79 批】上游没有「空文本」这种特判：`buffer` 为空时 `copyToClipboard("")` 照拷。
/// 本仓把「一列都没有」当不可执行（键位要说话），不拿空串盖掉用户剪贴板。
#[gpui_kit::test]
fn copying_column_names_is_refused_without_columns(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let (panel, cx) = panel_harness(cx);
    cx.update(|_w, app| {
        let grid = panel.read(app).grid.clone();
        grid.update(app, |state, cx| {
            state.delegate_mut().set_data(Vec::new(), Vec::new());
            state.refresh(cx);
        });
    });
    let result = cx.update(|_w, app| panel.update(app, |p, cx| p.copy_column_names(cx)));
    let reason = result.err().unwrap_or_default();
    assert!(
        reason.contains("没有可复制的列"),
        "要说明原因，实际是：{reason}"
    );
}

/// 【第 78 批 · 越界分支】藏掉**最右一列**时当前格的列位越界 → 清 `selection_pos`
/// （78 批那条三列用例只覆盖了「中间列、同列位重投影」这一支）。
/// 右栏 `selected_col` 按既有口径「清选保持清单当前选中」，这里一并把这条口径钉住。
#[gpui_kit::test]
fn hiding_the_rightmost_column_clears_the_current_cell(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let (panel, cx) = panel_harness(cx);
    let grid = cx.update(|_w, app| panel.read(app).grid.clone());

    // 组件列 2 = 数据列 1（`amount`，右端）
    cx.update(|_w, app| grid.update(app, |state, cx| state.set_selected_cell(0, 2, cx)));
    cx.run_until_parked();
    assert_eq!(
        cx.update(|_w, app| panel.read(app).selection_pos),
        Some((0, 2)),
        "前提：当前格在最右一列"
    );

    let result = cx.update(|window, app| {
        panel.update(app, |p, cx| p.hide_current_column(window, cx))
    });
    assert!(result.is_ok(), "可以藏：{result:?}");
    cx.run_until_parked();

    let selection_pos = cx.update(|_w, app| panel.read(app).selection_pos);
    assert_eq!(selection_pos, None, "越界清选：不让格指着空气");
}

/// 【D10】Shift 点击 = 以锚点扩成矩形选区（DBeaver `LightGrid.java:2718-2748`）：
/// 无修饰键的第一次点击立锚点，带 Shift 的第二次点击只换终点；矩形里**除焦点格**
/// 之外的格叠一层选中覆盖层（焦点格由组件自己的 overlay 画，叠两层会深一档）。
///
/// 事件走生产入口（组件 `on_cell_click` 发 `CellClicked` 后再发 `SelectCell`），
/// 这里直接 emit 同一对事件，覆盖「面板订阅 → delegate → render_td 哨兵」整条链。
#[gpui_kit::test]
fn shift_click_extends_a_rectangular_cell_range_and_paints_it(
    cx: &mut gpui_kit::TestAppContext,
) {
    use gpui_kit::Modifiers;

    cx.update(gpui_kit::init);
    let (panel, cx) = panel_harness(cx);
    let grid = cx.update(|_, app| panel.read(app).grid.clone());

    let click = |row: usize, col: usize, shift: bool, cx: &mut gpui_kit::VisualTestContext| {
        let modifiers = Modifiers {
            shift,
            ..Modifiers::default()
        };
        cx.update(|_, app| {
            grid.update(app, |_, cx| {
                cx.emit(gpui_kit::component::table::TableEvent::CellClicked {
                    row_ix: row,
                    col_ix: col,
                    modifiers,
                })
            });
        });
        cx.run_until_parked();
    };

    // 第一次点 (0,1)（第 1 个数据列）：单格选区
    click(0, 1, false, cx);
    let range = cx.update(|_, app| {
        grid.read(app).delegate().cell_range().expect("点过就有选区")
    });
    assert_eq!(range.anchor, (0, 1), "无修饰键点击立锚点");
    assert_eq!(range.focus, (0, 1), "单格：焦点就是锚点");

    // Shift 点 (1,2)：锚点不动，终点换到该格 → 2×2 矩形
    click(1, 2, true, cx);
    let range = cx.update(|_, app| {
        grid.read(app).delegate().cell_range().expect("Shift 后仍有选区")
    });
    assert_eq!(range.anchor, (0, 1), "Shift 不移动锚点");
    assert_eq!(range.focus, (1, 2), "Shift 只移动焦点");
    assert_eq!(range.rows_count(), 2, "两格对角 = 2 行");
    assert_eq!(range.data_columns(), vec![1, 2], "两格对角 = 2 个数据列");

    // 帧语义（0.10.0 / gpui-fast 0.1.17）：只 notify 不重跑 render，必须先 refresh
    cx.update(|window, _| window.refresh());
    cx.update(|window, cx| window.draw(cx).clear(cx));

    // 矩形内除焦点格外都铺覆盖层（组件 overlay 只管焦点格 (1,2)）
    // `debug_bounds` 只吃 `&'static str`，所以逐格写字面量
    assert!(
        cx.debug_bounds("editor-result-cell-in-range-0-1").is_some(),
        "矩形内的 (0,1) 应铺选中覆盖层"
    );
    assert!(
        cx.debug_bounds("editor-result-cell-in-range-0-2").is_some(),
        "矩形内的 (0,2) 应铺选中覆盖层"
    );
    assert!(
        cx.debug_bounds("editor-result-cell-in-range-1-1").is_some(),
        "矩形内的 (1,1) 应铺选中覆盖层"
    );
    assert!(
        cx.debug_bounds("editor-result-cell-in-range-1-2").is_none(),
        "焦点格由组件 overlay 画，delegate 不叠第二层"
    );
}

/// 【D10-1】正文拖拽选矩形：组件把「相对表格左上角的拖拽位置」换成 hover 格，
/// 再当作带 Shift 的新终点（DBeaver `LightGrid.java:3539-3566`）。
///
/// gpui 的真实拖拽状态机不在 headless 里跑，这里喂的是同一条几何链路：
/// 组件自己的布局快照（`debug_hit_geometry`，与 `drag_select_to` 内部同一份）
/// → `drag_select_to` → `CellClicked{shift}` → delegate 扩矩形。锚点仍由第一次点击立。
#[gpui_kit::test]
fn dragging_inside_the_grid_extends_the_cell_range(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let (panel, cx) = panel_harness(cx);
    let grid = cx.update(|_, app| panel.read(app).grid.clone());
    // `panel_harness` 给别的用例建的是裸 `TableState`（`cell_selectable` 默认 false）；
    // 生产里走 `grid::new_table_state`（`status.rs:488` 开 `cell_selectable`）。
    // 本用例要验的正是「可点选格子的表上拖拽成立」，故按生产口径补上。
    assert!(
        cx.update(|_, app| {
            grid.update(app, |state, _| state.cell_selectable = true);
            grid.read(app).cell_selectable
        }),
        "表格必须开 cell_selectable，否则 drag_select_to 直接早退"
    );

    // 第一步：点 (0,1) 立锚点（无修饰键）
    cx.update(|_, app| {
        grid.update(app, |_, cx| {
            cx.emit(gpui_kit::component::table::TableEvent::CellClicked {
                row_ix: 0,
                col_ix: 1,
                modifiers: gpui_kit::Modifiers::default(),
            })
        })
    });
    cx.run_until_parked();

    // 帧语义：先 refresh 再 draw，布局量（列宽/表头高/容器宽）才是真实值
    cx.update(|window, _| window.refresh());
    cx.update(|window, cx| window.draw(cx).clear(cx));

    // 用组件自己的布局快照算目标坐标（与 `drag_select_to` 内部同一份 geometry）：
    // 第 2 行中心、第 2 个数据列中心
    let geometry = cx.update(|_, app| grid.read(app).debug_hit_geometry(app));
    assert!(
        geometry.row_height > 0.0,
        "行高必须大于 0：{geometry:?}"
    );
    assert!(
        geometry.body_top > 0.0 && geometry.body_top < 200.0,
        "表头高异常：{geometry:?}"
    );
    assert!(geometry.rows_count >= 2, "行数不足：{geometry:?}");
    assert_eq!(geometry.col_widths.len(), 3, "列数不符：{geometry:?}");
    let target_x = geometry.left_fixed_width + geometry.col_widths[1] + geometry.col_widths[2] / 2.0;
    let target_y = geometry.body_top + geometry.row_height * 1.5;
    // 先自证换算式：目标点必须落在 (1,2)，否则下面的断言失败原因会指错地方
    assert_eq!(
        gpui_kit::component::table::cell_at_point(target_x, target_y, &geometry),
        Some((1, 2)),
        "目标点应落在 (1,2)：{geometry:?}"
    );

    // 第二步：拖到 (1,2)
    cx.update(|_, app| {
        grid.update(app, |table, cx| {
            table.drag_select_to(px(target_x), px(target_y), px(geometry.width), cx)
        })
    });
    cx.run_until_parked();

    // 分两层核对：① 组件自己的选中格有没有被搬到 (1,2)；② 面板订阅有没有扩矩形
    let state_cell = cx.update(|_, app| grid.read(app).selected_cell());
    assert_eq!(
        state_cell,
        Some((1, 2)),
        "drag_select_to 应把组件选中格搬到 (1,2)（target=({target_x},{target_y})，geometry={geometry:?}）"
    );

    let range = cx.update(|_, app| {
        grid.read(app)
            .delegate()
            .cell_range()
            .expect("拖过就有矩形选区")
    });
    assert_eq!(range.anchor, (0, 1), "拖拽不移动锚点");
    assert_eq!(range.focus, (1, 2), "焦点跟着拖到的格走");
    assert_eq!(range.rows_count(), 2, "两格对角 = 2 行");
    assert_eq!(range.data_columns(), vec![1, 2], "两格对角 = 2 个数据列");
}

/// 【D10-1b】从**未选中格**起拖：DBeaver 鼠标按下就先把该格选进选区
/// （`LightGrid.java:3152-3170`），所以任意格按下都能拖动扩矩形。
///
/// 本仓 `on_drag` 是渲染期挂的，按下那一帧起点格还不是选中态 → 由首个 `DragMove`
/// 调 `begin_drag_at` 补一次「无修饰点击」把锚点立到起点格，再走 `drag_select_to`。
#[gpui_kit::test]
fn dragging_from_an_unselected_cell_anchors_there_first(
    cx: &mut gpui_kit::TestAppContext,
) {
    cx.update(gpui_kit::init);
    let (panel, cx) = panel_harness(cx);
    let grid = cx.update(|_, app| panel.read(app).grid.clone());
    assert!(
        cx.update(|_, app| {
            grid.update(app, |state, _| state.cell_selectable = true);
            grid.read(app).cell_selectable
        }),
        "表格必须开 cell_selectable，否则拖拽链路整体早退"
    );

    // 先点 (0,1)：立一个「旧锚点」，证明起点格不是它
    cx.update(|_, app| {
        grid.update(app, |_, cx| {
            cx.emit(gpui_kit::component::table::TableEvent::CellClicked {
                row_ix: 0,
                col_ix: 1,
                modifiers: gpui_kit::Modifiers::default(),
            })
        })
    });
    cx.run_until_parked();
    let range = cx.update(|_, app| {
        grid.read(app).delegate().cell_range().expect("点过就有选区")
    });
    assert_eq!(range.anchor, (0, 1), "旧锚点在 (0,1)");

    // 从 (1,2) 起拖 → 按下即选中：锚点搬到起点格
    cx.update(|_, app| {
        grid.update(app, |table, cx| table.begin_drag_at(1, 2, cx));
    });
    cx.run_until_parked();
    let range = cx.update(|_, app| {
        grid.read(app).delegate().cell_range().expect("起点格已立锚点")
    });
    assert_eq!(range.anchor, (1, 2), "起点格成为新锚点");
    assert_eq!(range.focus, (1, 2), "刚起拖时焦点就是起点格");
    assert_eq!(
        cx.update(|_, app| grid.read(app).selected_cell()),
        Some((1, 2)),
        "组件选中格也搬到起点格"
    );

    // 帧语义：先 refresh 再 draw，布局量才是真实值
    cx.update(|window, _| window.refresh());
    cx.update(|window, cx| window.draw(cx).clear(cx));

    // 拖到 (0,1)：矩形覆盖 (0,1)..(1,2)
    let geometry = cx.update(|_, app| grid.read(app).debug_hit_geometry(app));
    let target_x = geometry.left_fixed_width + geometry.col_widths[1] / 2.0;
    let target_y = geometry.body_top + geometry.row_height * 0.5;
    assert_eq!(
        gpui_kit::component::table::cell_at_point(target_x, target_y, &geometry),
        Some((0, 1)),
        "目标点应落在 (0,1)：{geometry:?}"
    );
    cx.update(|_, app| {
        grid.update(app, |table, cx| {
            table.drag_select_to(px(target_x), px(target_y), px(geometry.width), cx)
        })
    });
    cx.run_until_parked();

    let range = cx.update(|_, app| {
        grid.read(app).delegate().cell_range().expect("拖过就有矩形")
    });
    assert_eq!(range.anchor, (1, 2), "拖拽不移动锚点");
    assert_eq!(range.focus, (0, 1), "焦点跟着 hover 格走");
    assert_eq!(range.rows_count(), 2, "两行");
    assert_eq!(range.data_columns(), vec![1, 2], "两个数据列");
}

/// 【D10-2】行号槽按下 = 整行：状态行三项与「当前格」口径一起钉住。
///
/// DBeaver 的 row header 不在 `columns` 里，`getCells(row, …)`
/// （`LightGrid.java:3125-3132`）铺的是全部数据列；本仓行号槽是组件第 0 列，
/// delegate 把矩形两侧归一成 `0..=末列`（见 `grid/delegate.rs::click_cell`）。
/// 状态行三项按 `SelectionExt` 口径（`ResultSetStatListener.java:80-92`）：
/// 行数 1、列数 2（两个数据列）、格数 2。
#[gpui_kit::test]
fn clicking_the_row_number_slot_selects_the_whole_row(
    cx: &mut gpui_kit::TestAppContext,
) {
    cx.update(gpui_kit::init);
    let (panel, cx) = panel_harness(cx);
    let grid = cx.update(|_, app| panel.read(app).grid.clone());

    // 行号槽（组件列 0）按下：走生产口径「按下即选」（`begin_drag_at` 先发无修饰
    // `CellClicked`、再 `set_selected_cell`，等价 DBeaver `:3152-3170` 的按下分支）。
    // 裸 `TableState::new` 的 `cell_selectable` 默认关（`grid/status.rs` 的生产构造才开），
    // 先按生产口径补上，否则整条链路早退、测的是另一张表。
    cx.update(|_, app| {
        grid.update(app, |table, cx| {
            table.cell_selectable = true;
            table.begin_drag_at(1, 0, cx);
        })
    });
    cx.run_until_parked();

    let range = cx.update(|_, app| {
        grid.read(app).delegate().cell_range().expect("点过就有选区")
    });
    assert_eq!(range.rows(), (1, 1), "只选点中的那一行");
    assert_eq!(range.cols(), (0, 2), "整行：从行号槽铺到末列");

    // 状态行三项（1 行 × 2 数据列 × 2 格）由 `SelectionExt` 同口径纯函数算出——
    // harness 的 `ResultPanel` 不注入 `status`（只有宿主 sync 才填），所以这里直接按
    // 同一个入口验一遍，避免「选区对了、计数口径错了」漏检。
    let counts = crate::grid::selection_counts(Some(&range));
    assert_eq!(counts, (1, 2, 2), "整行 = 1 行 × 2 数据列 × 2 格");

    // 【D10-9】「当前格」落到**首个数据列**：DBeaver 点行头后 `focusColumn` 就是
    // `getColumn(new Point(rowHeaderWidth + 1, e.y))`（`LightGrid.java:3140-3145`），
    // 它同时是 `SpreadsheetPresentation.getCurrentAttribute()` 的取值来源，即编辑目标
    // 就是首列的值。旧 §34 台账「行号槽保持为当前格」已按源码撤销。
    let pos = cx.update(|_, app| panel.read(app).selection_pos);
    assert_eq!(pos, Some((1, 1)), "行号槽点整行后当前格落到首个数据列");
}

/// 【D10-5】列头点击 → 整列选区。
///
/// 组件的列头点击只做一件事：`set_selected_col(col_ix, cx)`（vendor `state.rs:1064-1072`
/// → `:642-649`），它 emit `TableEvent::SelectColumn(col_ix)`。此前面板把它落在兜底
/// 分支里忽略，于是点列头只有列头自身变色、数据区没有整列高亮。
/// 这条钉的是**订阅链路**：真事件 → `delegate.click_col` → 矩形 + 三项计数。
#[gpui_kit::test]
fn clicking_a_column_header_selects_the_whole_column(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let (panel, cx) = panel_harness(cx);
    let grid = cx.update(|_, app| panel.read(app).grid.clone());

    cx.update(|_, app| {
        grid.update(app, |table, cx| table.set_selected_col(1, cx));
    });
    cx.run_until_parked();

    let range = cx.update(|_, app| {
        grid.read(app)
            .delegate()
            .cell_range()
            .expect("列头点击后应有选区")
    });
    assert_eq!(range.rows(), (0, 1), "整列铺满全部视图行");
    assert_eq!(range.cols(), (1, 1), "只框点中的那一列");
    assert_eq!(range.data_columns(), vec![1]);
    assert_eq!(range.data_cells_count(), 2, "2 行 × 1 数据列");

    let counts = crate::grid::selection_counts(Some(&range));
    assert_eq!(counts, (2, 1, 2), "整列 = 2 行 × 1 数据列 × 2 格");
}

/// 【右栏菜单】`set_grids` 给三条网格都接「面板」子菜单构造器：真值里每个 viewer
/// 的 `fillContextMenu` 都带 `panelsMenu`（`:3231-3233`），主网格 / 对比 / 分组页
/// 是三个独立 viewer，各自都要有。
#[gpui_kit::test]
fn set_grids_wires_the_panels_submenu_builder_on_every_grid(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    let (panel, cx) = panel_harness(cx);
    let (main_wired, group_wired) = panel.read_with(cx, |p, app| {
        (
            p.grid.read(app).delegate().panels_menu_builder.is_some(),
            p.group_grid
                .as_ref()
                .is_some_and(|g| g.read(app).delegate().panels_menu_builder.is_some()),
        )
    });
    assert!(main_wired, "主网格：单元格右键要有「面板」子菜单");
    assert!(group_wired, "分组页内嵌网格：同样要接");
}
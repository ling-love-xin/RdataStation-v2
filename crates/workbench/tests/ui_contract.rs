//! 工作台 UI 布局契约测试。
//!
//! 目的：把关键布局不变式固化成测试，防止后续改动**静默破坏**尺寸约束
//! （参考 navop 的布局契约测试实践）。
//!
//! 覆盖三类契约：
//! 1. **尺寸常量契约**：`ui.rs` 倍率与设计文档数值一致；
//! 2. **源码契约**：视图层不出现裸 `px(N.)` 与裸色值构造；
//! 3. **状态机契约**：边栏「完全隐藏 / 恢复」不丢失隐藏前模式。
//!
//! 数值来源：`docs/architecture/ui/ui-design-spec.md` §2.1。

use rds_workbench::SidebarMode;
use rds_workbench::view::toggle_hidden_mode;

/// 默认正文字号（`ui.rs` 倍率换算基准）。
const BASE_FONT_SIZE: f32 = 16.0;

/// 契约 1：结构尺寸倍率 × 16 必须等于设计值。
#[test]
fn ui_size_constants_match_design() {
    use rds_workbench::ui::*;

    let cases: &[(f32, f32, &str)] = &[
        (TITLE_BAR_HEIGHT, 36.0, "标题栏高度"),
        (ACTIVITY_BAR_WIDTH, 48.0, "活动栏宽度"),
        (ACTIVITY_ICON_SIZE, 28.0, "活动栏图标"),
        (ACTIVITY_ITEM_WIDTH, 44.0, "活动栏单项宽"),
        (ACTIVITY_ITEM_HEIGHT, 40.0, "活动栏单项高"),
        (LEFT_DOCK_WIDTH, 240.0, "左侧边栏起步宽"),
        (RIGHT_DOCK_WIDTH, 280.0, "右侧边栏起步宽"),
        (TITLE_LOGO_SIZE, 20.0, "标题栏图标"),
        (TITLE_SLOT_HEIGHT, 26.0, "挖空项目槽高"),
        (TITLE_SLOT_PADDING_X, 14.0, "挖空项目槽内距"),
        (QUICK_OPEN_ENTRY_WIDTH, 320.0, "Quick Open 入口宽"),
        (QUICK_OPEN_ENTRY_HEIGHT, 26.0, "Quick Open 入口高"),
        (QUICK_OPEN_PANEL_WIDTH, 560.0, "Quick Open 弹层宽"),
        (ROW_HEIGHT, 24.0, "列表/树行高"),
        (TREE_INDENT, 14.0, "树缩进步长"),
        (PANEL_HEADER_HEIGHT, 36.0, "面板头高度"),
        (CONTROL_HEIGHT_MD, 32.0, "标准控件高"),
        (CONTROL_HEIGHT_SM, 26.0, "小控件高"),
        (ICON_SIZE_SM, 14.0, "小图标"),
        (ICON_SIZE_MD, 16.0, "标准图标"),
    ];

    for (ratio, expected_px, label) in cases {
        assert_eq!(
            ratio * BASE_FONT_SIZE,
            *expected_px,
            "{label} 常量 {ratio}rem 偏离设计值 {expected_px}px（见 ui-design-spec §2.1）"
        );
    }
}

/// 契约 2a：视图层不得出现裸尺寸字面量。
///
/// 尺寸：结构尺寸走 `ui.rs` 常量（`rems(ui::…)` / `theme.font_size * ui::…`），
/// 局部间距走 Tailwind 尺度方法（`gap_1` / `px_2`）；裸 `px(N.)` 一律视为回归。
/// 注意：`h_px()` / `w_px()` 也会命中 `contains("px(")`（含 `px(` 子串），
/// 1px 细线请用 `ui::HAIRLINE`（本文件已不允许 `h_px()`）；同理 `.px(rems(…))` 也要改写成
/// `px_1()` / `px_2()` / `px_3()`（尺寸契约 #14 已把连接对话框模块纳入扫描）。
#[test]
fn view_layer_has_no_raw_size_literals() {
    let sources: &[(&str, &str)] = &[
        ("view.rs", include_str!("../src/view.rs")),
        ("panels.rs", include_str!("../src/panels.rs")),
        // 连接对话框模块（M3，#14 已清零；后续改动不得回退）
        (
            "connection_dialog/render.rs",
            include_str!("../src/components/connection_dialog/render.rs"),
        ),
        (
            "connection_dialog/helpers.rs",
            include_str!("../src/components/connection_dialog/helpers.rs"),
        ),
        (
            "connection_dialog/project_picker.rs",
            include_str!("../src/components/connection_dialog/project_picker.rs"),
        ),
        (
            "connection_dialog/managers.rs",
            include_str!("../src/components/connection_dialog/managers.rs"),
        ),
        (
            "connection_dialog/staging.rs",
            include_str!("../src/components/connection_dialog/staging.rs"),
        ),
        (
            "connection_dialog/mod.rs",
            include_str!("../src/components/connection_dialog/mod.rs"),
        ),
        // 编辑器 crate（A15 纳入扫描；UI 尺寸/颜色契约同样适用）
        ("editor/view/host.rs", include_str!("../../editor/src/view/host.rs")),
        (
            "editor/view/widgets/status_bar.rs",
            include_str!("../../editor/src/view/widgets/status_bar.rs"),
        ),
        (
            "editor/view/widgets/result_grid.rs",
            include_str!("../../editor/src/view/widgets/result_grid.rs"),
        ),
        (
            "editor/view/highlight.rs",
            include_str!("../../editor/src/view/highlight.rs"),
        ),
    ];

    for (name, src) in sources {
        assert!(
            !src.contains("px("),
            "{name} 出现裸 `px(...)` 尺寸字面量；结构尺寸请用 ui.rs 常量，局部间距用 Tailwind 尺度方法"
        );
    }
}

/// 契约 2b：UI 源码不得直接构造颜色，一律走主题 token（`cx.theme().colors.*`）。
///
/// 覆盖范围比尺寸契约宽（含对话框、项目视图、设置面板）：色值是硬约束，三者都已清零；
/// 存量欠债只有尺寸字面量（见 `connection-dialog-architecture.md` §14 #14）。
#[test]
fn ui_sources_have_no_raw_color_literals() {
    let sources: &[(&str, &str)] = &[
        ("workbench/view.rs", include_str!("../src/view.rs")),
        ("workbench/panels.rs", include_str!("../src/panels.rs")),
        (
            "connection_dialog/render.rs",
            include_str!("../src/components/connection_dialog/render.rs"),
        ),
        (
            "connection_dialog/helpers.rs",
            include_str!("../src/components/connection_dialog/helpers.rs"),
        ),
        (
            "connection_dialog/project_picker.rs",
            include_str!("../src/components/connection_dialog/project_picker.rs"),
        ),
        (
            "connection_dialog/managers.rs",
            include_str!("../src/components/connection_dialog/managers.rs"),
        ),
        (
            "connection_dialog/staging.rs",
            include_str!("../src/components/connection_dialog/staging.rs"),
        ),
        (
            "connection_dialog/mod.rs",
            include_str!("../src/components/connection_dialog/mod.rs"),
        ),
        ("project/ui.rs", include_str!("../../project/src/ui.rs")),
        (
            "settings/settings_view.rs",
            include_str!("../../settings/src/settings_view.rs"),
        ),
        // 编辑器 crate（A15 纳入扫描）
        ("editor/view/host.rs", include_str!("../../editor/src/view/host.rs")),
        (
            "editor/view/widgets/status_bar.rs",
            include_str!("../../editor/src/view/widgets/status_bar.rs"),
        ),
        (
            "editor/view/widgets/result_grid.rs",
            include_str!("../../editor/src/view/widgets/result_grid.rs"),
        ),
        (
            "editor/view/highlight.rs",
            include_str!("../../editor/src/view/highlight.rs"),
        ),
    ];

    for (name, src) in sources {
        assert!(
            !src.contains("rgb(") && !src.contains("hsla(") && !src.contains("Hsla::"),
            "{name} 出现裸色值构造；请从 `cx.theme().colors` 取色（透明用 transparent_black()）"
        );
    }
}

/// 契约 3：完全隐藏 / 恢复往返必须还原隐藏前模式（不丢失「收起」）。
#[test]
fn hidden_toggle_preserves_prior_mode() {
    // 收起 → 完全隐藏：记录快照为「收起」。
    let (mode, snapshot) = toggle_hidden_mode(SidebarMode::Collapsed, SidebarMode::Expanded);
    assert_eq!(mode, SidebarMode::Hidden, "首次切换应进入完全隐藏");
    assert_eq!(snapshot, SidebarMode::Collapsed, "快照应记录隐藏前模式");

    // 恢复：回到「收起」而非「展开」。
    let (mode, snapshot) = toggle_hidden_mode(mode, snapshot);
    assert_eq!(mode, SidebarMode::Collapsed, "恢复应还原隐藏前的收起状态");
    assert_eq!(snapshot, SidebarMode::Collapsed, "恢复后快照保持不变");

    // 展开 → 隐藏 → 恢复。
    let (mode, snapshot) = toggle_hidden_mode(SidebarMode::Expanded, SidebarMode::Collapsed);
    assert_eq!(mode, SidebarMode::Hidden);
    let (mode, _) = toggle_hidden_mode(mode, snapshot);
    assert_eq!(mode, SidebarMode::Expanded);
}

/// 契约 3（防御分支）：快照异常为 Hidden 时兜底展开，不把边栏再次藏起来。
#[test]
fn hidden_toggle_falls_back_to_expanded_on_bad_snapshot() {
    let (mode, _) = toggle_hidden_mode(SidebarMode::Hidden, SidebarMode::Hidden);
    assert_eq!(mode, SidebarMode::Expanded);
}

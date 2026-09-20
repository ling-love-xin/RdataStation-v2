//! 连接对话框**渲染状态矩阵**（降级路径）。
//!
//! 目标是覆盖“状态 × 渲染”的组合面，防止某一状态下的渲染分支无人触碰：
//! - 未选类型的空态引导条 → 填名称后消失 → 清空后复现（判据是表单内容，不是一次性标记）；
//! - 5 个 Tab（常规 / 网络 / 能力 / 驱动属性 / 高级）在**全局库未初始化**时全部可渲染；
//! - 作用域三态（仅全局 / 仅项目 / 全局+项目）切换后渲染；
//! - 结果行四个级别（Info / Success / Warning / Error）各渲染一帧；
//! - 暂存区固定高度：草稿累加到 13 条时区域高度**不得增长**（用户实测过“拉长页面”）。
//!
//! 本文件**不注入**全局库单例：走的就是“服务不可用”的降级路径（与 UI 首启、
//! 服务未就绪时的真实分支一致），也因此不会写到用户真实库。
//!
//! 注意：不使用 `use gpui_kit::*` / `use super::*` 通配导入——它们会把 gpui 的
//! `test` 属性宏带入作用域，导致 `#[gpui_kit::test]` 展开出的裸 `#[test]`
//! 解析到它自己，造成无限递归。所有依赖显式列举。

use std::cell::RefCell;
use std::rc::Rc;

use gpui_kit::component::Root;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{
    App, AppContext as _, Context, ElementId, Entity, IntoElement, ParentElement, Render,
    SharedString, Styled as _, TestAppContext, VisualTestContext, Window, div,
};

use rds_workbench::components::connection_dialog::{
    ConnectionDialogState, ResultLevel, ResultLine,
};
use rds_workbench::panels::{EditorPanel, Shared};

/// 测试宿主：与 `WorkbenchView` 同构（渲染面板 + 挂对话框层 + 宿主重绘桥）。
struct Harness {
    _shared: Shared,
    editor: Entity<EditorPanel>,
}

impl Harness {
    fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
        let shared = Shared::new();
        // 面板构造会读设置 global（`SettingsService::*`）：注入默认值，不读用户磁盘配置。
        if !cx.has_global::<settings::model::Settings>() {
            cx.set_global(settings::model::Settings::default());
        }
        let editor = cx.new(|cx| EditorPanel::new(shared.clone(), cx));
        let weak = cx.entity().downgrade();
        let bridge: Rc<dyn Fn(&mut App)> = Rc::new(move |cx: &mut App| {
            let _ = weak.update(cx, |_, cx| cx.notify());
        });
        *shared.host_redraw.borrow_mut() = Some(bridge);
        Self {
            _shared: shared,
            editor,
        }
    }

    /// 走生产入口打开「新建连接」对话框。
    ///
    /// 注意：调用点必须在 `Harness` 的 update 上下文**之外**（先取 `Entity<EditorPanel>`，
    /// 再单独 `update`）——面板入口会通过 `notify_host` 回调宿主重绘，嵌套 update 会重入 panic。
    fn editor(&self) -> Entity<EditorPanel> {
        self.editor.clone()
    }

    /// 面板持有的对话框状态（首次 open 后才有）。
    fn dialog(&self, cx: &App) -> Rc<ConnectionDialogState> {
        self.editor
            .read(cx)
            .dialog_state()
            .expect("对话框状态已创建")
    }
}

impl Render for Harness {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .child(self.editor.clone())
            .when_some(Root::render_dialog_layer(window, cx), |d, layer| {
                d.child(layer)
            })
    }
}

fn open_harness(cx: &mut TestAppContext) -> (Entity<Harness>, &mut VisualTestContext) {
    let slot: Rc<RefCell<Option<Entity<Harness>>>> = Rc::new(RefCell::new(None));
    let slot_in = slot.clone();
    let (_, cx) = cx.add_window_view(move |window, cx| {
        let harness = cx.new(|cx| Harness::new(window, cx));
        *slot_in.borrow_mut() = Some(harness.clone());
        Root::new(harness, window, cx)
    });
    let harness = slot.borrow().clone().expect("harness 已创建");
    (harness, cx)
}

/// 走生产入口打开对话框：先取面板实体，再单独 update（避开 `notify_host` → 宿主重绘的嵌套 update）。
fn open_new(harness: &Entity<Harness>, cx: &mut VisualTestContext) {
    let editor = cx.update(|_, cx| harness.read(cx).editor());
    cx.update(|window, cx| editor.update(cx, |e, cx| e.request_new_connection(window, cx)));
}

// ===== 观测快照（gpui-kit 的窗口测试 API）=====
//
// 对话框里的可断言元素一律 `.id(...)` + `.test_support()`（kit 观测），用 `find` 取快照：
// 它比 `debug_bounds` 强在两点——(1) `visible()` 能区分「不在元素树里」与「在树里但被裁掉 /
// 隐藏」，(2) 取不到时 panic 里带已登记路径（id 拼错一眼可见）。
// 注：这些元素在 headless 下点不到（鼠标事件到不了对话框的 deferred 子层），
// 所以快照只用于可见性与几何；真点击见 `helpers.rs::tests::header_interaction`。

/// 按 id 取快照（不在树里就 panic，带已登记路径）。
fn snap(cx: &mut VisualTestContext, id: &str) -> gpui_kit::test::ElementSnapshot {
    cx.update(|window, _| window.find(ElementId::Name(SharedString::from(id.to_string()))))
}

/// 元素是否**真可见**（在元素树里 + 没被裁掉 / 隐藏 / 透明）。
fn visible(cx: &mut VisualTestContext, id: &str) -> bool {
    cx.update(|window, _| {
        window
            .try_find(ElementId::Name(SharedString::from(id.to_string())))
            .is_some_and(|s| s.visible())
    })
}

/// 空态引导 / 五个 Tab / 作用域三态 / 结果行四级：逐一渲染不 panic。
#[gpui_kit::test]
fn dialog_state_matrix_renders_on_degraded_path(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (harness, cx) = open_harness(cx);
    open_new(&harness, cx);
    cx.update(|window, cx| window.draw(cx).clear(cx));

    // 1) 空态（未选类型 + 名称/地址为空）：首次引导条出现。
    assert!(
        visible(cx, "conn-general-guide"),
        "空态应渲染首次引导条（且真可见，不只是进了元素树）"
    );
    // 同时：类型树降级（目录为空）也要有交代——空白侧栏分不清“搜不到”与“没加载到”。
    assert!(
        visible(cx, "conn-type-empty"),
        "类型目录为空时应渲染空态提示"
    );
    let dialog = cx.update(|_, cx| harness.read(cx).dialog(cx));

    // 2) 填名称 → 引导条消失（老手版面不被占用）。
    cx.update(|window, cx| {
        dialog
            .name
            .update(cx, |s, cx| s.set_value("矩阵用例", window, cx));
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(!visible(cx, "conn-general-guide"), "名称非空后引导条应消失");

    // 3) 清空名称 → 引导条复现（判据是表单内容，不是"已展示过"标记）。
    cx.update(|window, cx| {
        dialog.name.update(cx, |s, cx| s.set_value("", window, cx));
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(visible(cx, "conn-general-guide"), "清空后引导条应复现");

    // 4) 五个 Tab 逐一渲染（类型 / 驱动 / 引用目录均为空的降级分支），
    //    并断言**行高锁定**：切 Tab 不得改变对话框高度（布局不跳动）。
    //    量的是两列行（`conn-body-row`）而不是内容区：内容区是滚动容器，它的
    //    快照落在滚动**内容**上（内容多高它多高，可大于视口——正常行为）。
    let mut row_height = None;
    for tab in 0..5 {
        dialog.active_tab.set(tab);
        cx.update(|window, cx| window.draw(cx).clear(cx));
        let height = snap(cx, "conn-body-row").bounds().size.height;
        match row_height {
            None => row_height = Some(height),
            Some(first) => assert_eq!(
                height, first,
                "Tab {tab}：两列行高应与首个 Tab 一致（切 Tab 不改变对话框高度）"
            ),
        }
    }

    // 5) 作用域三态：切换后各渲染一帧。
    for label in ["仅全局", "仅项目", "全局+项目"] {
        cx.update(|window, cx| {
            dialog.scope.update(cx, |s, cx| {
                s.set_selected_value(&SharedString::from(label), window, cx)
            });
        });
        cx.update(|window, cx| window.draw(cx).clear(cx));
    }

    // 6) 结果行四级：级别可读（UI 据此着色），渲染不 panic。
    for (level, summary) in [
        (ResultLevel::Info, "正在连接…"),
        (ResultLevel::Success, "连接成功 · 12 ms"),
        (ResultLevel::Warning, "已保存（分组未同步）"),
        (ResultLevel::Error, "保存失败: 名称为空"),
    ] {
        cx.update(|_, _cx| {
            *dialog.result.borrow_mut() = Some(ResultLine::new(level, summary));
        });
        cx.update(|window, cx| window.draw(cx).clear(cx));
        assert_eq!(
            cx.update(|_, _cx| dialog.result_level()),
            Some(level),
            "结果行级别应为 {level:?}"
        );
    }
    // 7) 两列同底 + **右列不得溢出**：
    //    ・侧栏高 = 行高（目录为空时也不得把对话框撑高）；
    //    ・右列的最后一个固定块（结果行）必须还在行内——旧实现里 Tab 内容区写死
    //      `rems(BODY_H)`，结果行被顶到行底之外 168px，底部一截被 Dialog 的
    //      body（`overflow_hidden`）裁掉，怎么滚都看不到。
    let row = snap(cx, "conn-body-row").bounds();
    let side = snap(cx, "conn-side-panel").bounds();
    let result = snap(cx, "conn-result-row").bounds();
    assert_eq!(
        side.size.height, row.size.height,
        "侧栏应填满行高（而不是按内容自适应）"
    );
    let row_bottom = row.origin.y + row.size.height;
    let result_bottom = result.origin.y + result.size.height;
    assert!(
        result_bottom <= row_bottom,
        "右列不得越出行高（行底 {row_bottom:?} / 结果行底 {result_bottom:?}）；越出即被裁掉"
    );
}

/// 暂存区固定高度：草稿累加不得拉长区域（用户实测过该回归）。
#[gpui_kit::test]
fn staging_area_keeps_fixed_height_with_many_drafts(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (harness, cx) = open_harness(cx);
    open_new(&harness, cx);
    cx.update(|window, cx| window.draw(cx).clear(cx));

    let h0 = snap(cx, "conn-staging-scroll").bounds().size.height;
    assert!(
        h0 > gpui_kit::Pixels::default(),
        "暂存区应有固定高度（当前 {h0:?}）"
    );

    let dialog = cx.update(|_, cx| harness.read(cx).dialog(cx));
    // 累加到 13 条（1 条初始空草稿 + 12 次「+ 添加」）。
    for _ in 0..12 {
        cx.update(|window, cx| dialog.staging_add(window, cx));
        cx.update(|_, cx| harness.update(cx, |_, cx| cx.notify()));
    }
    cx.update(|window, cx| window.draw(cx).clear(cx));

    let h1 = snap(cx, "conn-staging-scroll").bounds().size.height;
    assert_eq!(h1, h0, "草稿增多不得拉长暂存区（应为固定高度 + 内部滚动）");
    assert!(
        cx.update(|_, _cx| dialog.drafts.borrow().len()) >= 13,
        "草稿应已累加"
    );
}

/// 密码框：默认**掩码显示**，但 `value()` 必须仍是明文。
///
/// 这不是“好不好看”的问题：掩码一旦改掉取值语义，存下去的密码就是错的，
/// 而错误要等到连接时才发现（而且看起来像“凭据不对”）。
#[gpui_kit::test]
fn password_input_masks_display_without_hiding_the_value(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (harness, cx) = open_harness(cx);
    open_new(&harness, cx);
    let dialog = cx.update(|_, cx| harness.read(cx).dialog(cx));

    cx.update(|window, cx| {
        dialog
            .pass
            .update(cx, |s, cx| s.set_value("s3cret-pw", window, cx));
    });
    let (masked, value) = cx.update(|_, cx| {
        let input = dialog.pass.read(cx);
        // `is_masked` 在输入状态的表现层（`presentation()`）上：掩码是**渲染语义**，
        // 不是取值语义——这正是本用例要钉住的区别。
        (input.presentation().is_masked(), input.value().to_string())
    });
    assert!(masked, "密码框应默认掩码显示（输入框里不该出现明文）");
    assert_eq!(value, "s3cret-pw", "掩码只影响渲染：取值必须仍是明文");
}

/// 键盘选型路径（决策 #107）：焦点在类型列表上时，↑↓ 选行、Enter 选中该类型。
///
/// 这是把自绘行换成 `List` 组件的核心收益——自绘行 Tab 到不了、也不收 ↑↓。
/// （鼠标点击行也会聚焦列表，随后同样能键盘操作。）
#[gpui_kit::test]
fn type_list_is_keyboard_navigable(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (harness, cx) = open_harness(cx);
    open_new(&harness, cx);
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let dialog = cx.update(|_, cx| harness.read(cx).dialog(cx));
    // 注入目录（不依赖服务）：MySQL 有启用驱动、SQLite 没有。
    cx.update(|_, _cx| {
        *dialog.types.borrow_mut() = vec![
            ds_type("mysql", "MySQL", "relational"),
            ds_type("sqlite", "SQLite", "file-based"),
        ];
        *dialog.drivers.borrow_mut() = vec![driver("mysql", "mysql", "MySQL (sqlx)", true)];
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));

    let list = cx
        .update(|_, _cx| dialog.type_tree.borrow().clone())
        .expect("类型列表应已在 open() 里建好");
    // 聚焦列表（等价于鼠标点了一下行）。
    cx.update(|window, cx| list.update(cx, |st, cx| st.focus(window, cx)));
    cx.update(|window, cx| window.draw(cx).clear(cx));

    // ↓ 选中第一行（MySQL）→ Enter 确认；再 ↓ 到第二行（SQLite，无启用驱动）→ Enter。
    cx.simulate_keystrokes("down");
    cx.simulate_keystrokes("enter");
    assert_eq!(
        cx.update(|_, _cx| dialog.selected_type.borrow().clone()),
        "mysql",
        "↑↓ + Enter 应能选中类型（键盘路径）"
    );
    cx.simulate_keystrokes("down");
    cx.simulate_keystrokes("enter");
    assert_eq!(
        cx.update(|_, _cx| dialog.selected_type.borrow().clone()),
        "mysql",
        "无可用驱动的类型：键盘确认也不改选中，但结果行给原因"
    );
    let msg = cx
        .update(|_, _cx| dialog.result_summary())
        .unwrap_or_default();
    assert!(msg.contains("暂无可用驱动"), "应给出原因：{msg}");
}

fn ds_type(
    id: &str,
    name: &str,
    category: &str,
) -> engine::persistence::driver_store::DataSourceType {
    engine::persistence::driver_store::DataSourceType {
        id: id.into(),
        name: name.into(),
        category: category.into(),
        icon: Some("🗄".into()),
        enabled: true,
        created_at: String::new(),
    }
}

fn driver(
    id: &str,
    type_id: &str,
    name: &str,
    enabled: bool,
) -> engine::persistence::driver_store::Driver {
    engine::persistence::driver_store::Driver {
        id: id.into(),
        type_id: type_id.into(),
        name: name.into(),
        driver_kind: "native".into(),
        is_file: false,
        default_port: None,
        url_template: None,
        download_url: None,
        download_checksum: None,
        version: None,
        config_schema: String::new(),
        supported_auth_types: None,
        capabilities: None,
        driver_properties: None,
        enabled,
    }
}

/// 出几帧让布局稳定。
///
/// 首帧与稳定帧的内容可能不同（引导条 / 目录回填等都在 render 里判），
/// 拿首帧的量当基线会得到偏差几帧才收敛的位置——做几何对比前先稳住。
fn draw_frames(cx: &mut VisualTestContext, n: usize) {
    for _ in 0..n {
        cx.update(|window, cx| window.draw(cx).clear(cx));
    }
}

/// 推进组件内 spring（揭示动效）直到收敛。
///
/// headless 下时钟不会自己走（`spring` 取的是 `background_executor().now()`，只有显式推进才动），
/// 所以必须“推时钟 + 出帧”成对做：只 `draw` 不推时钟，让步永远停在原处。
fn settle_reveal(cx: &mut VisualTestContext) {
    for _ in 0..60 {
        cx.update(|window, cx| {
            cx.background_executor()
                .advance_clock(std::time::Duration::from_millis(16));
            window.draw(cx).clear(cx);
        });
    }
}

/// 分组折叠（决策 #108）：标题行是语义 `Button` + `Collapsible`（组件内 spring 揭示动效）。
///
/// 钉住三件事：
/// 1. 标题行是**可交互的真元素**：被 kit 的观测登记（`.id()` + `.test_support()`）、当前可见、
///    行宽 == 面板内宽（按钮 `w_full` + `px_0`，不是只包住 chevron 与标题）；
/// 2. 折叠是**几何**上的——正文不可见（仍挂载）+ 下一个分组的标题行上移（布局真的变短），
///    不是只翻一个状态位；
/// 3. **只动被切换的那个分组**，再展开原样复原。
///
/// 这里用 `toggle_section` 驱动而不是坐标点击：headless 下鼠标事件到不了对话框层
/// （本仓第二次撞到，同款注记见 `analytics_resource/tests/dialog_window.rs` 与
/// `editor/src/view/tests.rs`；已实测：同样结构的行在**普通窗口**里一点就中，在对话框里点不中），
/// 而 `toggle_section` 正是标题行 `Button::on_click` 里调用的同一个方法。
/// “点击 / 悬停 / 键盘的真观感”归真机验收（用户指南 §9.1 F 段）；
/// 点击→回调这一段接线由 `helpers.rs` 的 `header_interaction` 窗口用例盯住（那里没有对话框层）。
#[gpui_kit::test]
fn outline_section_collapses_from_its_whole_title_row(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (harness, cx) = open_harness(cx);
    open_new(&harness, cx);
    // 先出几帧让布局收敛（实测前 4 帧内容还在长高，第 5 帧起稳定）。
    draw_frames(cx, 8);
    let dialog = cx.update(|_, cx| harness.read(cx).dialog(cx));

    let is_collapsed = |cx: &mut VisualTestContext, id: &'static str| {
        cx.update(|_, _cx| dialog.section_collapsed(id))
    };
    // 下一个分组的标题行底边（“布局真的变短了”的直接证据）。
    let next_row_bottom = |cx: &mut VisualTestContext| {
        let b = snap(cx, "conn-sec-org").bounds();
        b.origin.y + b.size.height
    };

    // 标题行：观察快照与命中测试同一套登记（`.id()` + `.test_support()`），
    // 和“只给 debug_bounds 看的锚点”不同——它是真元素，真机上也点得中。
    let row = snap(cx, "conn-sec-conn");
    assert!(row.visible(), "标题行应可见");
    let panel_width = row.bounds().size.width;
    assert!(!is_collapsed(cx, "conn"), "默认应为展开");
    assert!(visible(cx, "conn-sec-body-conn"), "展开时正文应可见");
    let expanded_bottom = next_row_bottom(cx);

    // 收起：状态 + 几何（spring 收敛后正文被夹成不可见、后面的分组上移）。
    cx.update(|_, _| dialog.toggle_section("conn"));
    settle_reveal(cx);
    assert!(is_collapsed(cx, "conn"), "收起后状态应为折叠");
    assert!(
        !visible(cx, "conn-sec-body-conn"),
        "收起后正文应被揭示进度夹成不可见"
    );
    assert!(
        snap(cx, "conn-sec-body-conn").bounds().size.width > gpui_kit::Pixels::default(),
        "收起只是夹高度：正文仍挂载（宽度还在），而不是被删掉重建"
    );
    let folded_bottom = next_row_bottom(cx);
    assert!(
        folded_bottom < expanded_bottom,
        "收起后后面的分组应上移（布局真的变短）：{expanded_bottom:?} → {folded_bottom:?}"
    );
    assert_eq!(
        snap(cx, "conn-sec-conn").bounds().size.width,
        panel_width,
        "标题行宽度不因折叠而变（整行仍是按钮）"
    );
    assert!(
        !is_collapsed(cx, "org"),
        "只该收起被切换的那个分组（旁边的分组仍是展开态）"
    );
    // 下面这句同时是断言：取不到快照就 panic（相邻分组不得因折叠被移除）。
    // 注意：`org` 在滚动区之下，`visible()` 为假（被视口裁掉）——所以判据是它的**位置**，
    // 不是可见性（这正是 `find` 比 `debug_bounds` 多出来的信息：旧写法看不出来这一点）。
    let _org_row = snap(cx, "conn-sec-org");

    // 再展开：回到原高（揭示动效是可逆的，不是只能单向收起）。
    cx.update(|_, _| dialog.toggle_section("conn"));
    settle_reveal(cx);
    assert!(!is_collapsed(cx, "conn"), "再点应展开");
    assert!(visible(cx, "conn-sec-body-conn"), "展开后正文应重新可见");
    let restored_bottom = next_row_bottom(cx);
    assert_eq!(
        restored_bottom, expanded_bottom,
        "展开后应回到原高度（后面的分组回到原位）"
    );
}

/// 草稿列表键盘路径（决策 #107）：焦点在草稿列表里时 ↑↓ 直接切换条目
/// （列表自己的绑定比对话框容器级的 DraftPrev/DraftNext 更深，不会两边都动）。
#[gpui_kit::test]
fn draft_list_is_keyboard_navigable(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (harness, cx) = open_harness(cx);
    open_new(&harness, cx);
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let dialog = cx.update(|_, cx| harness.read(cx).dialog(cx));
    // 加一条草稿 → 共两条，光标在最后一条。
    cx.update(|window, cx| dialog.staging_add(window, cx));
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert_eq!(cx.update(|_, _cx| dialog.draft_cursor.get()), 1);

    let list = cx
        .update(|_, _cx| dialog.draft_list.borrow().clone())
        .expect("草稿列表应已在 open() 里建好");
    cx.update(|window, cx| list.update(cx, |st, cx| st.focus(window, cx)));
    cx.update(|window, cx| window.draw(cx).clear(cx));

    cx.simulate_keystrokes("up");
    assert_eq!(
        cx.update(|_, _cx| dialog.draft_cursor.get()),
        0,
        "↑ 应切到上一条草稿（无需再按 Enter）"
    );
    cx.simulate_keystrokes("down");
    assert_eq!(
        cx.update(|_, _cx| dialog.draft_cursor.get()),
        1,
        "↓ 应切回下一条草稿"
    );
}

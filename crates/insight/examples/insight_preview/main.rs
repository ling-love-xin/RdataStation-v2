//! 洞察模块预览工具：独立窗口 + 假数据 + 真主题 + 真图标。
//!
//! ```sh
//! cargo run -p rds-insight --example insight_preview
//! ```
//!
//! ## 它解决什么
//!
//! headless 窗口测试只能断言「不 panic、结构不回归」——**它看不到像素**。而洞察的
//! UI 问题恰恰集中在像素层：窄窗溢出、长表名撑破工具栏、卡片不换行、列宽不够时
//! 整列不可达。本工具起一个真窗口把这些摆到眼前。
//!
//! ## 与测试的分工（不可替代）
//!
//! | | 窗口测试 | 本工具 |
//! | --- | --- | --- |
//! | 驱动 | headless `draw(cx)` | 真窗口 + 事件循环 |
//! | 能验 | 不 panic / 状态流转 | 布局 / 截断 / 换行 / 滚动 / 真交互 |
//! | 断言 | 有 | 无（靠人眼） |
//!
//! ## 怎么用
//!
//! 1. 顶栏选**场景**（十种形态，见 [`Scenario`]）；
//! 2. 拨**宽度档位**到 368 / 480 —— 这两档对应 1024 / 1152 窗口下中央编辑区的真实
//!    可用宽，绝大多数挤出 / 溢出问题在这里才现身；
//! 3. 中间区里的东西是**可以点的**：切 Tab、勾对比、展开折叠，与产品里一致。
//!
//! ## 为什么要有宽度档位
//!
//! 洞察在工作台里不是占满窗口的一整屏，而是被外壳三面夹住：
//! `可用宽 = 窗口宽 − activity 48 − 左 dock 288 − 右 dock 320`。
//! 1280 窗口下只有 624px。若预览把面板铺满全宽，窄栏问题会被整体掩盖。
//!
//! ## 宿主
//!
//! 面板遵循 D20「自己不取数」：预览工具扮演宿主，用 `InsightView::set_*` 直接推
//! 载荷。**没有连接、没有 DuckDB、不写产品数据根**，起停都在毫秒级。

mod scenarios;

use std::path::PathBuf;

use gpui_kit::base::{Disableable as _, StyledExt as _};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{ActiveTheme, Root, Sizable as _, Theme, ThemeRegistry};
use gpui_kit::*;

use rds_insight::InsightWorkspacePanel;

use scenarios::Scenario;

/// 预览宽度档位（px）。
///
/// `None` = 铺满（看宽窗表现 / 做截图）。其余按「外壳三面夹住后的中央可用宽」取，
/// 并标出对应的模拟窗口宽——验收时照着这个数字对，而不是凭感觉。
const WIDTH_PRESETS: [(Option<f32>, &str); 6] = [
    (Some(368.0), "368px · ≈1024 窗口"),
    (Some(480.0), "480px · ≈1152 窗口"),
    (Some(624.0), "624px · ≈1280 窗口"),
    (Some(784.0), "784px · ≈1440 窗口"),
    (Some(960.0), "960px · ≈1600 窗口"),
    (None, "铺满"),
];

/// 默认落在 624px（最常见的 1280 窗口下中央可用宽）。
const DEFAULT_WIDTH_IX: usize = 2;

/// 预览用的数据根（仓库 `.rds/tmp/` 下，与项目约定一致：过程产物不入库）。
fn preview_home() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../.rds/tmp/insight-preview")
}

fn main() {
    // 数据根：必须在任何线程 / 运行时启动前设好。面板本身不落盘，但要阻止
    // store / 规则索引按「可执行文件所在目录」解析，把产物散进 target/。
    //
    // SAFETY: edition 2024 下 `set_var` 为 unsafe——此处尚在 main 第一条语句，
    // 进程内没有任何其它线程，不存在数据竞争。
    unsafe {
        std::env::set_var("RDS_HOME", preview_home().join("home"));
    }
    std::fs::create_dir_all(preview_home()).ok();

    // 大栈线程：与主程序同口径。GPUI 的视图树构建 / 布局 / 事件派发在 debug 构建下
    // 递归较深，Windows 主线程默认 1 MiB 栈会直接溢出崩溃（表现为窗口一闪就没）。
    // `RDS_UI_STACK_MB` 可覆盖（debug 下「点一下就崩」时用它先判定是不是栈不够）。
    let stack_mb = std::env::var("RDS_UI_STACK_MB")
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .filter(|mb| *mb >= 16)
        .unwrap_or(64);
    let handle = std::thread::Builder::new()
        .name("insight-preview-main".to_string())
        .stack_size(stack_mb * 1024 * 1024)
        .spawn(run_preview)
        .expect("failed to spawn preview main thread");
    if handle.join().is_err() {
        eprintln!("[insight-preview] 主线程异常退出");
        std::process::exit(1);
    }
}

fn run_preview() {
    gpui_kit::application()
        // 资产源不注册 = 所有图标静默渲染为空（元素在但看不见）：预览里会把「布局错」
        // 和「图标没加载」混在一起看。这里直接用组件库自带的全量图标集。
        .with_assets(gpui_kit::assets::AllAssets)
        .run(|cx| {
            gpui_kit::init(cx);
            // 产品主题：与工作台同一份 `assets/themes/*.json`。
            // 不看真主题，颜色类问题会全部失真。
            load_product_themes(cx);

            let _ = cx.open_window(WindowOptions::default(), |window, cx| {
                window.set_rem_size(px(16.));
                let panel = cx.new(|cx| InsightWorkspacePanel::new(cx));
                // 首帧就落在最有代表性的形态上：表画像（右栏五卡 + 中央 EDA 都在）
                Scenario::TableProfile.apply(&panel, cx);
                let harness = cx.new(|_cx| PreviewHarness {
                    panel,
                    scenario: Scenario::TableProfile,
                    width_ix: DEFAULT_WIDTH_IX,
                });
                // 窗口第一层必须是 component 的 Root（不是 base 的）：
                // 对话框 / 弹层 / 主题都挂在它上面。
                cx.new(|cx| Root::new(harness, window, cx))
            });
        });
}

/// 预览窗口：顶栏（场景 + 宽度）+ 中间工作页面板。
struct PreviewHarness {
    panel: Entity<InsightWorkspacePanel>,
    scenario: Scenario,
    width_ix: usize,
}

impl PreviewHarness {
    /// 校验 / 纠正越界索引：`WIDTH_PRESETS` 改长度时不必同步这里。
    fn clamped_width_ix(&self) -> usize {
        self.width_ix.min(WIDTH_PRESETS.len() - 1)
    }
}

impl Render for PreviewHarness {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity();
        let colors = cx.theme().colors;
        let width_ix = self.clamped_width_ix();
        let scenario = self.scenario;
        let (maybe_width, width_label) = WIDTH_PRESETS[width_ix];

        // ---- 第一行：场景按钮（自己也会换行——那正好是布局压力的一部分）----
        let mut scenario_row = div().flex_none().w_full().h_flex().flex_wrap().gap_1();
        for item in Scenario::ALL {
            let active = item == scenario;
            scenario_row = scenario_row.child(
                Button::new(SharedString::from(format!(
                    "preview-scenario-{}",
                    Scenario::ALL
                        .iter()
                        .position(|s| *s == item)
                        .unwrap_or_default()
                )))
                .ghost()
                .xsmall()
                .toggled(active)
                .label(item.label())
                .tooltip(item.hint())
                .on_click({
                    let entity = entity.clone();
                    move |_, _, app| {
                        // 先取面板实体再复位 / 重灌：任意顺序切换都不串上一个场景的状态。
                        //
                        // 这里刻意不用 `let Some(panel) = … else { return }`：let-else 里的
                        // reborrow 会一路延伸到闭包末尾（`&mut App` 被降级成 `&App`），
                        // 后面的 `update` 就拿不到可变借用了。
                        let panel = entity.read(app).panel.clone();
                        Scenario::reset(&panel, app);
                        item.apply(&panel, app);
                        let _ = entity.update(app, |harness, cx| {
                            harness.scenario = item;
                            cx.notify();
                        });
                    }
                }),
            );
        }

        // ---- 第二行：宽度档位 + 当前场景说明 ----
        let meta_row = div()
            .flex_none()
            .w_full()
            .h_flex()
            .items_center()
            .gap_2()
            .child(
                Button::new("preview-width-narrower")
                    .ghost()
                    .xsmall()
                    .label("◀ 窄")
                    .disabled(width_ix == 0)
                    // 内联闭包而不是抽 Helper：事件类型（`ClickEvent`）由形参位置
                    // 推断即可，抽成函数就得把协程治理的第三个参数写出来——那是
                    // 凭印象写 API，不如交给编译器。
                    .on_click({
                        let entity = entity.clone();
                        move |_, _, app| {
                            let _ = entity.update(app, |harness, cx| {
                                let ix = harness.clamped_width_ix() as i32 - 1;
                                harness.width_ix = ix.max(0) as usize;
                                harness.width_ix = harness.clamped_width_ix();
                                cx.notify();
                            });
                        }
                    }),
            )
            .child(
                div()
                    .flex_none()
                    .min_w_0()
                    .text_xs()
                    .text_color(colors.muted_foreground)
                    .child(format!("可用宽 {}", width_label)),
            )
            .child(
                Button::new("preview-width-wider")
                    .ghost()
                    .xsmall()
                    .label("宽 ▶")
                    .disabled(width_ix + 1 >= WIDTH_PRESETS.len())
                    .on_click({
                        let entity = entity.clone();
                        move |_, _, app| {
                            let _ = entity.update(app, |harness, cx| {
                                let ix = harness.clamped_width_ix() + 1;
                                harness.width_ix = ix.min(WIDTH_PRESETS.len() - 1);
                                cx.notify();
                            });
                        }
                    }),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_xs()
                    .text_color(colors.muted_foreground)
                    .text_ellipsis()
                    .child(scenario.hint()),
            );

        // ---- 主区：按档位宽度渲染中间工作页 ----
        //
        // 面板外层套一个**确定宽度**的盒子，右边留白 dull-background 表示「工作台上这块
        // 地方之外还有别的东西」。铺满档位为 None → flex_1。
        let mut stage = div().h_full().flex_none();
        stage = match maybe_width {
            Some(w) => stage.w(px(w)).border_r_1().border_color(colors.border),
            None => stage.flex_1().min_w_0(),
        };

        div()
            .size_full()
            .v_flex()
            .items_stretch()
            .bg(colors.background)
            .child(
                div()
                    .flex_none()
                    .w_full()
                    .v_flex()
                    .gap_1()
                    .px_2()
                    .py_1()
                    .border_b_1()
                    .border_color(colors.border)
                    .child(scenario_row)
                    .child(meta_row),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .h_flex()
                    .items_stretch()
                    .overflow_hidden()
                    .child(stage.child(self.panel.clone()))
                    // 档位宽度之外的留白：让「面板就这么宽」这件事看得出来
                    .child(div().flex_1().min_w_0().h_full().bg(colors.muted)),
            )
    }
}

/// 加载产品主题（`assets/themes/*.json`），与工作台启动时同一份文件。
///
/// 读不到时静默回落 gpui-kit 默认主题：预览工具不该因为主题文件缺失就起不来，
/// 但会在 stderr 说清楚，免得把「主题没加载」误当成布局问题。
fn load_product_themes(cx: &mut App) {
    let themes_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/themes");
    let Ok(entries) = std::fs::read_dir(&themes_dir) else {
        eprintln!(
            "[insight-preview] 主题目录不可用 {}：回落 gpui-kit 默认主题",
            themes_dir.display()
        );
        return;
    };
    let registry = ThemeRegistry::global_mut(cx);
    for entry in entries.flatten() {
        let path = entry.path();
        // 产品语义 token 与键名白名单都不是主题文件（结构不同），预览里不加载（与 app 层同口径）
        if path.extension().and_then(|s| s.to_str()) != Some("json")
            || matches!(
                path.file_name().and_then(|s| s.to_str()),
                Some("product-tokens.json" | "theme-keys.json")
            )
        {
            continue;
        }
        match std::fs::read_to_string(&path) {
            Ok(content) => {
                if let Err(e) = registry.load_themes_from_str(&content) {
                    eprintln!("[insight-preview] 主题解析失败 {}: {e}", path.display());
                }
            }
            Err(e) => eprintln!("[insight-preview] 主题读取失败 {}: {e}", path.display()),
        }
    }
    // 接上产品主题：与 `app` 的 `attach_rds_theme` 同一做法（明/暗各取一份）
    let (light, dark) = {
        let registry = ThemeRegistry::global(cx);
        (
            registry.themes().get("RDS Light").cloned(),
            registry.themes().get("RDS Dark").cloned(),
        )
    };
    if let (Some(light), Some(dark)) = (light, dark) {
        let theme = cx.global_mut::<Theme>();
        theme.light_theme = light;
        theme.dark_theme = dark;
    }
}

//! Quick Open 宿主装配：标题栏入口控件、浮层懒创建与挂载、动作副作用落地。
//!
//! 从 `view.rs` 搬入（重构 S3）：Quick Open 的四段宿主代码集中于此，
//! `view.rs` 只留 `toggle_quick_open` 与三处调用（入口点击 / 键位 Action / overlay 挂载）。
//! 浮层本体（输入 / 结果 / 键盘通道 / 防抖与回填）在 [`super::palette`]；
//! `Shared::quick_open` 是全应用一份的权威开关。
//! 规格：`docs/architecture/quick_open/quick-open-prototype-design.md`。

use std::rc::Rc;

use gpui_kit::base::StyledExt as _;
use gpui_kit::component::{Icon, IconName, Theme};
use gpui_kit::*;

use crate::quick_open::model::{Action, Mode};
use crate::quick_open::palette::{QuickOpenHost, QuickOpenPalette};
use crate::ui;
use crate::view::{WorkbenchView, restore_snapshot};

impl WorkbenchView {
    /// 标题栏搜索条（点击唤起，`Ctrl+P` 见 commands.rs 绑定）：与快捷键同一入口——
    /// 打开、清空上次输入、聚焦输入框；真正输入在浮层里（单一权威输入）。
    pub(crate) fn render_quick_open_entry(
        &self,
        entity: &Entity<Self>,
        theme: &Theme,
    ) -> AnyElement {
        let qo_entity = entity.clone();
        div()
            .id("quick-open-entry")
            .role(gpui::Role::Button)
            .test_support()
            .h_flex()
            .items_center()
            .gap_2()
            .w(rems(ui::QUICK_OPEN_ENTRY_WIDTH))
            .h(rems(ui::QUICK_OPEN_ENTRY_HEIGHT))
            .px_2()
            .rounded_sm()
            .bg(theme.colors.border)
            .cursor_pointer()
            .child(Icon::new(IconName::Search).size_3p5())
            .child(
                div()
                    .text_xs()
                    .text_color(theme.colors.muted_foreground)
                    .child("搜索连接、命令…"),
            )
            .child(
                div()
                    .ml_auto()
                    .text_xs()
                    .text_color(theme.colors.muted_foreground)
                    .child("Ctrl+P"),
            )
            .on_click(move |_, window, app| {
                qo_entity.update(app, |this, cx| {
                    let _ = window;
                    this.toggle_quick_open(Mode::Default, cx)
                });
            })
            // 2026-10-19 修复：gpui-kit TitleBar 把整个 bar（含全部 children）标记为
            // `WindowControlArea::Drag`，Windows 无边框窗口下点击命中 HTCAPTION 会被
            // 系统当作标题栏拖动吞掉，on_click 永远收不到（主题按钮“无法切换”根因）。
            // `.occlude()` 让本元素的 hitbox 挡掉后方（父 bar）的 Drag hitbox，命中时
            // 不再返回 HTCAPTION，点击正常派发。
            .occlude()
            .into_any_element()
    }

    /// 懒创建浮层（首帧渲染；输入框与结果列表由浮层自己按需创建）。
    pub(crate) fn ensure_quick_open_palette(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Entity<QuickOpenPalette> {
        if let Some(palette) = self.quick_open_palette.clone() {
            return palette;
        }
        let host: Rc<dyn QuickOpenHost> = Rc::new(WorkbenchQuickOpenHost {
            view: cx.entity().downgrade(),
        });
        let shared = self.shared.clone();
        let palette = cx.new(|cx| QuickOpenPalette::new(shared, host, cx));
        self.quick_open_palette = Some(palette.clone());
        palette
    }

    /// 挂载浮层（未打开时返回 `None`，overlay 不进元素树）。
    pub(crate) fn render_quick_open(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.shared.quick_open.get() {
            return None;
        }
        let palette = self.ensure_quick_open_palette(cx);
        if self.quick_open_open_pending {
            self.quick_open_open_pending = false;
            palette.update(cx, |palette, cx| {
                palette.on_opened(self.quick_open_mode_pending, cx)
            });
        }
        Some(palette.into_any_element())
    }

    /// 执行一条结果（浮层经宿主端口回传；动作语义只在这里实现）。
    pub(crate) fn execute_quick_open_action(
        &mut self,
        action: Action,
        keep_open: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        tracing::info!(action = ?action, keep_open, "【探针】execute_quick_open_action：命令面板执行一条动作");
        match action {
            Action::NewDocument(mode) => self.new_editor_document(mode, window, cx),
            Action::ShowProperties(request) => {
                // 与导航搜索结果同一去向：中央 Dock 属性 tab（请求已带全定位信息）。
                self.shared.show_properties(*request, window, cx);
            }
            Action::RevealMetadata(object) => {
                // 与「打开左面板」同一口径（切到数据源 + 展开），再把请求交给导航面板：
                // 展开链路与选中都在那边做，工作台不重复一份“树长什么样”。
                self.shared.active_left.set(crate::view::LeftPanel::Database);
                self.shared.left_mode.set(crate::view::SidebarMode::Expanded);
                self.shared.request_reveal(object);
            }
            Action::OpenLeftPanel(panel) => {
                // 与右 dock `Shared::open_right_panel` 同一口径：先登记进 `left_open_panels`
                //（tab 条只显示点过的模块；漏登记 → dock 已存在时新模块永不挂载）。
                {
                    let mut open = self.shared.left_open_panels.borrow_mut();
                    if !open.contains(&panel) {
                        open.push(panel);
                    }
                }
                self.shared.active_left.set(panel);
                self.shared.left_mode.set(crate::view::SidebarMode::Expanded);
                if panel == crate::view::LeftPanel::Resources {
                    // 与活动栏点击同一口径：激活即取一次列表（M6）。
                    self.request_resources_refresh(cx);
                }
                // 状态改完由宿主这拍同步 dock：挂载 / 激活 active 面板（apply_left_mode
                // 的已存在分支负责补齐未挂载 tab 并切到激活项）。
                self.apply_left_mode(window, cx);
            }
            Action::OpenRightPanel(panel) => {
                let open_before = self
                    .shared
                    .right_open_panels
                    .borrow()
                    .iter()
                    .map(|p| format!("{p:?}"))
                    .collect::<Vec<_>>()
                    .join(",");
                // 与右活动栏点击同一口径（Mock 的候选清单与生成历史在展开这一拍重读）。
                self.shared.open_right_panel(panel, cx);
                // 与 `OpenLeftPanel` 对称：状态改完由宿主这拍同步 dock（挂载 / 激活 / 展开）。
                // 旧实现只委托 `open_right_panel`，布局同步依赖 `notify_host` 的 defer 重绘——
                // 弱引用升级失败或事件时序错位时右 dock 会停在收起态，表现为「命令点了进不去面板」。
                self.apply_right_mode(window, cx);
                let open_after = self
                    .shared
                    .right_open_panels
                    .borrow()
                    .iter()
                    .map(|p| format!("{p:?}"))
                    .collect::<Vec<_>>()
                    .join(",");
                tracing::info!(
                    target = "rds_workbench::quick_open",
                    panel = format!("{panel:?}"),
                    right_mode = format!("{:?}", self.shared.right_mode.get()),
                    open_before = open_before,
                    open_after = open_after,
                    "命令面板：打开右侧面板"
                );
            }
            Action::OpenSettings => {
                self.open_settings(window, cx);
            }
            Action::HideSidebars => {
                let s = &self.shared;
                if s.left_mode.get() != crate::view::SidebarMode::Hidden {
                    s.left_mode_before_hidden.set(s.left_mode.get());
                    s.left_mode.set(crate::view::SidebarMode::Hidden);
                }
                if s.right_mode.get() != crate::view::SidebarMode::Hidden {
                    s.right_mode_before_hidden.set(s.right_mode.get());
                    s.right_mode.set(crate::view::SidebarMode::Hidden);
                }
            }
            Action::RestoreSidebars => {
                let s = &self.shared;
                if s.left_mode.get() == crate::view::SidebarMode::Hidden {
                    s.left_mode
                        .set(restore_snapshot(s.left_mode_before_hidden.get()));
                }
                if s.right_mode.get() == crate::view::SidebarMode::Hidden {
                    s.right_mode
                        .set(restore_snapshot(s.right_mode_before_hidden.get()));
                }
            }
            Action::SelectConnection(ix) => {
                // 与侧边栏点击一致：清导航缓存，防止串数据；不自动连接。
                self.shared.selected.set(Some(ix));
                self.shared.invalidate_nav_cache();
            }
            Action::OpenDocument(path) => {
                // 与草稿箱面板双击同一条路（先存请求，宿主 render 消费时落到编辑器）：
                // 可写 / 只读由编辑器按路径自己判定，这里不重复一份“哪些路径只读”的口径。
                self.shared.request_open_in_editor(path);
            }
        }
        if !keep_open {
            self.shared.quick_open.set(false);
        }
        cx.notify();
    }
}

/// Quick Open 的宿主端口：把浮层的动作落回工作台。
///
/// 只有工作台知道这些副作用怎么做（新建文档要 `Window`、资产库要顺带刷新、
/// 连接选中要清导航缓存）；浮层因此不依赖 `WorkbenchView`，可在窗口测试里用哑宿主驱动。
struct WorkbenchQuickOpenHost {
    /// 宿主体（浮层与工作台同生命周期；升级失败 = 视图已销毁，动作静默丢弃）。
    view: WeakEntity<WorkbenchView>,
}

impl QuickOpenHost for WorkbenchQuickOpenHost {
    fn execute(&self, action: Action, keep_open: bool, window: &mut Window, cx: &mut App) {
        let Some(view) = self.view.upgrade() else {
            return;
        };
        view.update(cx, |view, cx| {
            view.execute_quick_open_action(action, keep_open, window, cx)
        });
    }
}

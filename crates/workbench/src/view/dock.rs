//! 左右 Dock 布局、三模式同步、标题栏与活动栏。
//!
//! 自 `view.rs` 拆分（行为不变）。

use super::*;
impl WorkbenchView {
    /// 真面板化装配：只挂**已打开过**的模块 tab（`Shared::left_open_panels`）。
    /// 空列表兜底为当前激活模块（启动早期 / 状态被清空的防御）。
    pub(super) fn left_dock_layout(&self, cx: &Context<Self>) -> Option<DockLayout> {
        // 类型交给推断：`panel_handle` 返回 `Arc<dyn base::PanelView>`。
        let mut handles = Vec::new();
        let open = self.shared.left_open_panels.borrow();
        for panel in open.iter() {
            match panel {
                LeftPanel::Draft => {
                    if let Some(tab) = &self.left_scratch_tab {
                        handles.push(panel_handle(tab.clone()));
                    }
                }
                LeftPanel::Database => {
                    if let Some(tab) = &self.left_nav_tab {
                        handles.push(panel_handle(tab.clone()));
                    }
                }
                LeftPanel::Resources => {
                    if let Some(tab) = &self.left_resource_tab {
                        handles.push(panel_handle(tab.clone()));
                    }
                }
                LeftPanel::Plugin => {
                    if let Some(tab) = &self.left_plugin_tab {
                        handles.push(panel_handle(tab.clone()));
                    }
                }
            }
        }
        drop(open);
        if handles.is_empty() {
            // 兜底：空列表时至少挂当前激活模块（各 arm 同构为 `Option<Arc<dyn PanelView>>`）。
            let active = self.shared.active_left.get();
            let tab = match active {
                LeftPanel::Draft => self.left_scratch_tab.clone().map(panel_handle),
                LeftPanel::Database => self.left_nav_tab.clone().map(panel_handle),
                LeftPanel::Resources => self.left_resource_tab.clone().map(panel_handle),
                LeftPanel::Plugin => self.left_plugin_tab.clone().map(panel_handle),
            };
            if let Some(handle) = tab {
                handles.push(handle);
            }
        }
        if handles.is_empty() {
            return None;
        }
        let mut layout = DockLayout::tabs();
        for handle in handles {
            layout = layout.panel_view(handle, cx);
        }
        Some(layout)
    }

    pub(crate) fn apply_left_mode(&self, window: &mut Window, cx: &mut Context<Self>) {
        let mode = self.shared.left_mode.get();
        let (Some(area), Some(layout)) = (self.area.clone(), self.left_dock_layout(cx)) else {
            return;
        };
        // Hidden → Expanded 重建 dock 时，把当前激活模块设为 dock 初始 tab。
        // 这里同时留句柄：dock 已存在时要把 active 面板**挂载并激活**（与右 dock
        // 同一补丁——`Shared::open_left_panel` 等无窗口入口只改状态，dock 同步必须
        // 由宿主这拍完成，否则命令面板切不过去 tab）。
        let active_tab = match self.shared.active_left.get() {
            LeftPanel::Draft => self.left_scratch_tab.clone().map(panel_handle),
            LeftPanel::Database => self.left_nav_tab.clone().map(panel_handle),
            LeftPanel::Resources => self.left_resource_tab.clone().map(panel_handle),
            LeftPanel::Plugin => self.left_plugin_tab.clone().map(panel_handle),
        };
        let active_id = active_tab.as_ref().map(|handle| handle.panel_id(cx));
        area.update(cx, |area, cx| {
            // DockLayout 不可 clone：Expanded / Collapsed 两分支互斥，各自消费 layout。
            if mode == SidebarMode::Expanded {
                if !area.has_dock(DockPlacement::Left) {
                    area.set_dock(DockPlacement::Left, layout, window, cx);
                    // 起步宽度取 `ui::LEFT_DOCK_WIDTH`（18rem，基准字号下 288px，随界面缩放；
                    // layout-design §2.3，用户拖拽可调）。数值以 ui.rs 常量为准，此处不再写死 px。
                    area.set_dock_size(
                        DockPlacement::Left,
                        cx.theme().font_size * ui::LEFT_DOCK_WIDTH,
                        window,
                        cx,
                    );
                    if let Some(id) = active_id.clone() {
                        area.select_panel(id, window, cx);
                    }
                } else if !area.is_dock_open(DockPlacement::Left) {
                    area.toggle_dock(DockPlacement::Left, window, cx);
                } else if let (Some(handle), Some(id)) = (active_tab, active_id) {
                    // 面板化后左 dock 是动态 tab 集：active 模块未挂载 → 挂载（自动激活）；
                    // 已挂载但未激活 → 切激活。`select_panel` / `add_panel_view` 幂等。
                    if area.panel(id.clone()).is_none() {
                        area.add_panel_view(handle.clone(), DockPlacement::Left, None, window, cx);
                    } else {
                        area.select_panel(id.clone(), window, cx);
                    }
                }
            } else if mode == SidebarMode::Collapsed {
                if !area.has_dock(DockPlacement::Left) {
                    area.set_dock(DockPlacement::Left, layout, window, cx);
                    area.set_dock_size(
                        DockPlacement::Left,
                        cx.theme().font_size * ui::LEFT_DOCK_WIDTH,
                        window,
                        cx,
                    );
                }
                if area.is_dock_open(DockPlacement::Left) {
                    area.toggle_dock(DockPlacement::Left, window, cx);
                }
            } else if area.has_dock(DockPlacement::Left) {
                area.remove_dock(DockPlacement::Left, window, cx);
            }
        });
    }

    /// 右 dock 装配（与 `left_dock_layout` 同构：只挂已打开的模块 tab）。
    pub(super) fn right_dock_layout(&self, cx: &Context<Self>) -> Option<DockLayout> {
        // 类型交给推断：`panel_handle` 返回 `Arc<dyn base::PanelView>`。
        let mut handles = Vec::new();
        let open = self.shared.right_open_panels.borrow();
        for panel in open.iter() {
            match panel {
                RightPanel::Insight => {
                    if let Some(tab) = &self.right_insight_tab {
                        handles.push(panel_handle(tab.clone()));
                    }
                }
                RightPanel::Mock => {
                    if let Some(tab) = &self.right_mock_tab {
                        handles.push(panel_handle(tab.clone()));
                    }
                }
                RightPanel::History => {
                    if let Some(tab) = &self.right_history_tab {
                        handles.push(panel_handle(tab.clone()));
                    }
                }
                RightPanel::Archive => {
                    if let Some(tab) = &self.right_archive_tab {
                        handles.push(panel_handle(tab.clone()));
                    }
                }
            }
        }
        drop(open);
        if handles.is_empty() {
            // 兜底：空列表时至少挂当前激活模块（各 arm 同构为 `Option<Arc<dyn PanelView>>`）。
            let active = self.shared.active_right.get();
            let tab = match active {
                RightPanel::Insight => self.right_insight_tab.clone().map(panel_handle),
                RightPanel::Mock => self.right_mock_tab.clone().map(panel_handle),
                RightPanel::History => self.right_history_tab.clone().map(panel_handle),
                RightPanel::Archive => self.right_archive_tab.clone().map(panel_handle),
            };
            if let Some(handle) = tab {
                handles.push(handle);
            }
        }
        if handles.is_empty() {
            return None;
        }
        let mut layout = DockLayout::tabs();
        for handle in handles {
            layout = layout.panel_view(handle, cx);
        }
        Some(layout)
    }

    pub(crate) fn apply_right_mode(&self, window: &mut Window, cx: &mut Context<Self>) {
        let mode = self.shared.right_mode.get();
        let (Some(area), Some(layout)) = (self.area.clone(), self.right_dock_layout(cx)) else {
            return;
        };
        // Hidden → Expanded 重建 dock 时，把当前激活模块设为 dock 初始 tab。
        // 这里同时留句柄：dock 已存在时要把 active 面板**挂载并激活**（见下方
        // Expanded 分支的补丁——`Shared::open_right_panel` 等无窗口入口只改状态，
        // dock 同步必须由宿主这拍完成，否则 tab 切不过去）。
        let active_tab = match self.shared.active_right.get() {
            RightPanel::Insight => self.right_insight_tab.clone().map(panel_handle),
            RightPanel::Mock => self.right_mock_tab.clone().map(panel_handle),
            RightPanel::History => self.right_history_tab.clone().map(panel_handle),
            RightPanel::Archive => self.right_archive_tab.clone().map(panel_handle),
        };
        let active_id = active_tab.as_ref().map(|handle| handle.panel_id(cx));
        area.update(cx, |area, cx| {
            // DockLayout 不可 clone：Expanded / Collapsed 两分支互斥，各自消费 layout。
            if mode == SidebarMode::Expanded {
                if !area.has_dock(DockPlacement::Right) {
                    area.set_dock(DockPlacement::Right, layout, window, cx);
                    // 起步宽度 280px（= 17.5rem 基准）。
                    area.set_dock_size(
                        DockPlacement::Right,
                        cx.theme().font_size * ui::RIGHT_DOCK_WIDTH,
                        window,
                        cx,
                    );
                    if let Some(id) = active_id.clone() {
                        area.select_panel(id, window, cx);
                    }
                } else if !area.is_dock_open(DockPlacement::Right) {
                    area.toggle_dock(DockPlacement::Right, window, cx);
                }
                // 面板化后右 dock 是动态 tab 集：active 模块未挂载 → 挂载（自动激活）；
                // 已挂载但未激活 → 切激活。`select_panel` / `add_panel_view` 幂等，
                // render 每帧调用无副作用（与 `apply_left_mode` 的 set_dock 同先例）。
                if let (Some(handle), Some(id)) = (&active_tab, &active_id) {
                    if area.panel(id.clone()).is_none() {
                        area.add_panel_view(handle.clone(), DockPlacement::Right, None, window, cx);
                    } else {
                        area.select_panel(id.clone(), window, cx);
                    }
                }
            } else if mode == SidebarMode::Collapsed {
                if !area.has_dock(DockPlacement::Right) {
                    area.set_dock(DockPlacement::Right, layout, window, cx);
                    area.set_dock_size(
                        DockPlacement::Right,
                        cx.theme().font_size * ui::RIGHT_DOCK_WIDTH,
                        window,
                        cx,
                    );
                }
                if area.is_dock_open(DockPlacement::Right) {
                    area.toggle_dock(DockPlacement::Right, window, cx);
                }
            } else if area.has_dock(DockPlacement::Right) {
                area.remove_dock(DockPlacement::Right, window, cx);
            }
        });
    }

    // ===== 活动栏 ↔ dock tab 双向同步（事件路径，另一半在模块 tab 的 `set_active`） =====

    /// 左活动栏点击 → 打开对应模块：
    /// 1. 登记进 `left_open_panels`（tab 条只显示点过的模块）；
    /// 2. 已在 dock → `select_panel` 切激活；不在 → `add_panel_view` 挂载（自动激活）；
    /// 3. dock 不存在（Hidden）→ 不动作，render 的 `apply_left_mode` 按 open 列表重建。
    pub(super) fn open_left_panel(
        &self,
        panel: LeftPanel,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        {
            let mut open = self.shared.left_open_panels.borrow_mut();
            if !open.contains(&panel) {
                open.push(panel);
            }
        }
        let Some(area) = self.area.clone() else {
            return;
        };
        // 各 tab 类型不同：match 统一返回 `(handle, id)` 元组（类型一致）。
        let (handle, id) = match panel {
            LeftPanel::Draft => match self.left_scratch_tab.clone() {
                Some(tab) => {
                    let handle = panel_handle(tab);
                    let id = handle.panel_id(cx);
                    (handle, id)
                }
                None => return,
            },
            LeftPanel::Database => match self.left_nav_tab.clone() {
                Some(tab) => {
                    let handle = panel_handle(tab);
                    let id = handle.panel_id(cx);
                    (handle, id)
                }
                None => return,
            },
            LeftPanel::Resources => match self.left_resource_tab.clone() {
                Some(tab) => {
                    let handle = panel_handle(tab);
                    let id = handle.panel_id(cx);
                    (handle, id)
                }
                None => return,
            },
            LeftPanel::Plugin => match self.left_plugin_tab.clone() {
                Some(tab) => {
                    let handle = panel_handle(tab);
                    let id = handle.panel_id(cx);
                    (handle, id)
                }
                None => return,
            },
        };
        area.update(cx, |area, cx| {
            if area.panel(id).is_some() {
                // 已打开过：切 tab（dock 关闭时也改树，render 打开后即正确）。
                area.select_panel(id, window, cx);
            } else if area.has_dock(DockPlacement::Left) {
                // 首次打开：挂载进左 dock 第一个 tab 组（`add_panel_view` 自带激活）。
                area.add_panel_view(handle, DockPlacement::Left, None, window, cx);
            }
        });
    }

    /// 右活动栏点击 → 打开对应模块（同上，右侧 dock）。
    pub(super) fn open_right_panel(
        &self,
        panel: RightPanel,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        {
            let mut open = self.shared.right_open_panels.borrow_mut();
            if !open.contains(&panel) {
                open.push(panel);
            }
        }
        let Some(area) = self.area.clone() else {
            return;
        };
        // 各 tab 类型不同：match 统一返回 `(handle, id)` 元组（类型一致）。
        let (handle, id) = match panel {
            RightPanel::Insight => match self.right_insight_tab.clone() {
                Some(tab) => {
                    let handle = panel_handle(tab);
                    let id = handle.panel_id(cx);
                    (handle, id)
                }
                None => return,
            },
            RightPanel::Mock => match self.right_mock_tab.clone() {
                Some(tab) => {
                    let handle = panel_handle(tab);
                    let id = handle.panel_id(cx);
                    (handle, id)
                }
                None => return,
            },
            RightPanel::History => match self.right_history_tab.clone() {
                Some(tab) => {
                    let handle = panel_handle(tab);
                    let id = handle.panel_id(cx);
                    (handle, id)
                }
                None => return,
            },
            RightPanel::Archive => match self.right_archive_tab.clone() {
                Some(tab) => {
                    let handle = panel_handle(tab);
                    let id = handle.panel_id(cx);
                    (handle, id)
                }
                None => return,
            },
        };
        area.update(cx, |area, cx| {
            if area.panel(id).is_some() {
                area.select_panel(id, window, cx);
            } else if area.has_dock(DockPlacement::Right) {
                area.add_panel_view(handle, DockPlacement::Right, None, window, cx);
            }
        });
    }

    // ===== 标题栏 =====
    // 差异点：无窗口标题文本。承载 gpui-kit 官方 TitleBar（对应 layout-design.md §2.1）：
    // - 窗口拖拽（Drag hitbox，Windows 系统级）、双击最大化、窗口控制按钮（─ □ ✕）由组件自带；
    // - 高度覆盖为 36px、背景覆盖为主题 title_bar 纯色（对齐 layout-proposal.html v5）。

    /// 快速布局预设（UI 先行，未接线）：仅提供菜单外观与文案，点击暂不改变布局状态。
    /// 接线时每项应一次性写入 `left_mode`/`right_mode`（复用 `apply_*_mode` 同步到 dock）。
    const QUICK_LAYOUT_PRESETS: &[(&str, &str)] = &[
        ("left", "仅左栏"),
        ("right", "仅右栏"),
        ("both", "双栏"),
        ("full", "全屏"),
    ];

    pub(super) fn render_title_bar(
        &self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let theme = cx.theme();
        let pt = settings::product_tokens::get(cx);
        let entity = cx.entity();

        // 软件图标随当前实际明暗主题取 light/ 或 dark/ 下的 32px PNG：System 策略已在
        // Theme::change 阶段解析为 Light/Dark，这里以 theme.is_dark() 为准（与取色同源）。
        // 路径走 `paths::assets_dir()`：发布包取可执行文件同级 `assets/`，开发期回退仓库那份。
        let icon_dir = if theme.is_dark() { "dark" } else { "light" };
        let icon_path = paths::assets_dir()
            .join("icons")
            .join(icon_dir)
            .join("32x32.png");
        let logo = img(icon_path.as_path()).w_5().h_5().rounded_sm();

        // 挖空项目槽：产品语义 token `title_bar.slot.background`。
        // 项目名取自当前项目会话（P0）；未打开项目时显示占位。
        let has_project = self.shared.project.borrow().is_some();
        let project_name = self
            .shared
            .project
            .borrow()
            .as_ref()
            .map(|s| s.name.clone())
            .unwrap_or_else(|| "未打开项目".to_string());
        // 触发器用语义控件（`Selectable` + 键盘可达由 `Button` 自带），不自己画可点 div。
        // 2026-10-19：随标题栏调高到 28px；加文件夹图标 + 1px 边框，让“挖空”槽在
        // 标题栏上有明确的可视轮廓（此前只有背景色，显得“显示不清楚”）。
        let slot = Button::new("project-slot")
            .ghost()
            .h(rems(ui::TITLE_SLOT_HEIGHT))
            .px_3p5()
            .rounded_md()
            .border_1()
            .border_color(theme.colors.border)
            .bg(pt.title_bar_slot_background(theme))
            .child(
                div()
                    .h_flex()
                    .items_center()
                    .gap_2()
                    .child(Icon::new(CatalogIcon::Folder).size_3p5())
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.colors.muted_foreground)
                            .child("项目"),
                    )
                    .child(
                        div()
                            .text_xs()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(theme.colors.foreground)
                            .child(project_name),
                    ),
            );

        // M1：有项目时项目槽本身就是菜单触发器（`Button::dropdown_menu`）——方向键导航 /
        // Enter / Escape / 点击外部关闭、焦点恢复都由语义组件负责，宿主不再持有 `menu_open`
        // 之类的开关状态（组件内部状态才是权威）。菜单内容由 `project` crate 提供。
        let slot: AnyElement = if has_project {
            let host = self.project_host().clone();
            let inputs = self
                .project_inputs
                .clone()
                .expect("project inputs lazy init");
            slot.dropdown_menu(move |menu, _, _| {
                project::ui::build_project_menu(menu, &host, &inputs)
            })
            .into_any_element()
        } else {
            slot.into_any_element()
        };

        // Quick Open 入口（构建在 `quick_open/host.rs`；点击唤起，Ctrl+P 见 commands.rs 绑定）：
        // 与快捷键同一入口——打开、清空上次输入、聚焦输入框；真正输入在浮层里（单一权威输入）。
        let quick_open = self.render_quick_open_entry(&entity, &theme);

        // 主题切换入口：图标随当前外观策略（暗→月、亮→日、跟随系统→显示器），
        // 点击走与 Ctrl+Shift+L 同一条命令路径（`ToggleThemeMode`）。
        let appearance = SettingsService::theme_mode(cx);
        let theme_icon = match appearance {
            settings::model::AppearanceMode::Dark => CatalogIcon::Moon,
            settings::model::AppearanceMode::System => CatalogIcon::Monitor,
            settings::model::AppearanceMode::Light => CatalogIcon::Sun,
        };
        let theme_toggle = Button::new("theme-toggle")
            .ghost()
            .icon(Icon::new(theme_icon).size_3p5())
            .tooltip("切换明暗主题 (Ctrl+Shift+L)")
            .on_click(|_, window, app| {
                SettingsService::toggle_theme_mode(Some(window), app);
            });
        // 标题栏可点击控件统一包 `occlude()`：从父 bar 的 `WindowControlArea::Drag`
        // 命中区剔除（见 quick-open-entry 注释），否则 Windows 上点击被系统吞掉。
        let theme_toggle = div()
            .h_flex()
            .flex_none()
            .items_center()
            .occlude()
            .child(theme_toggle);

        // 语言切换入口：先做入口与写库，翻译覆盖是渐进的（`tr()` 逐处替换）。
        let lang_switch = Button::new("lang-switch")
            .ghost()
            .icon(Icon::new(CatalogIcon::Languages).size_3p5())
            .tooltip("界面语言")
            .dropdown_menu(|menu, _, _| {
                let mut menu = menu.item(PopupMenuItem::label("界面语言"));
                menu = menu.item(PopupMenuItem::separator());
                menu = menu.item(PopupMenuItem::new("简体中文").on_click(|_, window, app| {
                    SettingsService::set_locale("zh-CN", Some(window), app);
                }));
                menu.item(PopupMenuItem::new("English").on_click(|_, window, app| {
                    SettingsService::set_locale("en-US", Some(window), app);
                }))
            });
        let lang_switch = div()
            .h_flex()
            .flex_none()
            .items_center()
            .occlude()
            .child(lang_switch);

        // 三栏布局：左右 flex_1 占位对称，Quick Open 严格居中；
        // 右侧窗口控制按钮（─ □ ✕）由 TitleBar 自带渲染，无需自绘。
        TitleBar::new()
            .h(rems(ui::TITLE_BAR_HEIGHT))
            .pl_2p5()
            .bg(theme.colors.title_bar)
            .child(
                div()
                    .flex_1()
                    .h_flex()
                    .items_center()
                    .gap_2()
                    .child(logo)
                    .child(
                        div()
                            .h_flex()
                            .flex_none()
                            .items_center()
                            .occlude()
                            .child(slot),
                    ),
            )
            .child(quick_open)
            .child(div().flex_1())
            .child(theme_toggle)
            .child(lang_switch)
            .child(
                div()
                    .h_flex()
                    .flex_none()
                    .items_center()
                    .occlude()
                    .child(self.render_quick_layout_button(cx)),
            )
    }

    // 快速布局下拉：布局图标 + 预设菜单。已接线——每项一次性写入左右 dock 模式
    // （复用 `apply_left_mode`/`apply_right_mode` 同步到 dock，见 `apply_quick_layout`）。
    pub(super) fn render_quick_layout_button(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity();
        Button::new("quick-layout")
            .ghost()
            .icon(IconName::LayoutDashboard)
            .tooltip("快速布局")
            .dropdown_menu(move |menu, _, _| {
                let mut menu = menu.item(PopupMenuItem::label("布局预设"));
                menu = menu.item(PopupMenuItem::separator());
                for (key, label) in Self::QUICK_LAYOUT_PRESETS.iter().copied() {
                    let entity = entity.clone();
                    menu = menu.item(PopupMenuItem::new(label).on_click(move |_, window, app| {
                        entity.update(app, |this, cx| {
                            this.apply_quick_layout(key, window, cx);
                        });
                    }));
                }
                menu
            })
    }

    /// 快速布局预设接线：一次性写入左右 dock 模式（复用 `apply_*_mode` 同步）。
    pub(super) fn apply_quick_layout(
        &self,
        key: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use SidebarMode::*;
        let (lm, rm) = match key {
            "left" => (Expanded, Collapsed),
            "right" => (Collapsed, Expanded),
            "both" => (Expanded, Expanded),
            // 全屏：双栏收起（活动栏保留，dock 以 strip 收边，可再点开）。
            "full" => (Collapsed, Collapsed),
            _ => return,
        };
        if self.shared.left_mode.get() != lm {
            self.shared.left_mode.set(lm);
        }
        if self.shared.right_mode.get() != rm {
            self.shared.right_mode.set(rm);
        }
        self.apply_left_mode(window, cx);
        self.apply_right_mode(window, cx);
    }

    // ===== 活动栏 =====

    pub(super) fn render_left_activity_bar(&self, cx: &mut Context<Self>) -> AnyElement {
        let active = self.shared.active_left.get();
        let shared = self.shared.clone();
        let entity = cx.entity();

        let mut menu = SidebarMenu::new();
        for panel in LeftPanel::ALL {
            let selected = active == panel;
            let panel_clone = panel;

            let item = SidebarMenuItem::new(panel.label())
                .icon(panel.icon())
                .active(selected)
                .on_click({
                    // 每个回调各自 clone：on_click 与 context_menu 都 move 捕获，
                    // 共享同一份 clone 会触发 E0382（partially moved value）。
                    let shared = shared.clone();
                    let entity = entity.clone();
                    move |_, window, app| {
                        let mode = shared.left_mode.get();
                        let activating = !(mode == SidebarMode::Expanded
                            && shared.active_left.get() == panel_clone);
                        if !activating {
                            shared.left_mode.set(SidebarMode::Collapsed);
                        } else {
                            shared.active_left.set(panel_clone);
                            shared.left_mode.set(SidebarMode::Expanded);
                        }
                        let activating_resources =
                            activating && panel_clone == LeftPanel::Resources;
                        entity.update(app, |this, cx| {
                            if activating {
                                this.open_left_panel(panel_clone, window, cx);
                            }
                            if activating_resources {
                                this.request_resources_refresh(cx);
                            }
                            cx.notify();
                        });
                    }
                })
                .context_menu({
                    let shared = shared.clone();
                    let entity = entity.clone();
                    move |menu, _, _| {
                        let mut menu = menu.item(PopupMenuItem::label("左侧栏"));
                        menu = menu.item(PopupMenuItem::separator());
                        let shared = shared.clone();
                        let entity = entity.clone();
                        menu = menu.item(PopupMenuItem::new("隐藏侧边栏").on_click({
                            let shared = shared.clone();
                            let entity = entity.clone();
                            move |_, _, app| {
                                shared.left_mode.set(SidebarMode::Hidden);
                                entity.update(app, |_, cx| cx.notify());
                            }
                        }));
                        menu.item(PopupMenuItem::new("收起侧边栏").on_click({
                            let shared = shared.clone();
                            let entity = entity.clone();
                            move |_, _, app| {
                                shared.left_mode.set(SidebarMode::Collapsed);
                                entity.update(app, |_, cx| cx.notify());
                            }
                        }))
                    }
                });
            menu = menu.child(item);
        }

        let settings_shared = shared.clone();
        let settings_entity = entity.clone();
        let footer = div().h_flex().w_full().p_2().justify_center().child(
            Button::new("left-settings")
                .icon(Icon::new(IconName::Settings))
                .ghost()
                .on_click(move |_, window, app| {
                    let entity = settings_entity.clone();
                    entity.update(app, |this, cx| this.open_settings(window, cx));
                    settings_shared.quick_open.set(false);
                }),
        );

        Sidebar::new("left-activity")
            .side(Side::Left)
            .collapsible(SidebarCollapsible::Icon)
            .collapsed(true)
            .child(menu)
            .footer(footer)
            .into_any_element()
    }

    pub(super) fn render_right_activity_bar(&self, cx: &mut Context<Self>) -> AnyElement {
        let active = self.shared.active_right.get();
        let shared = self.shared.clone();
        let entity = cx.entity();

        let mut menu = SidebarMenu::new();
        for panel in RightPanel::ALL {
            let selected = active == panel;
            let panel_clone = panel;

            let item = SidebarMenuItem::new(panel.label())
                .icon(panel.icon())
                .active(selected)
                .on_click({
                    // 同左栏：每个回调各自 clone，避免 move 冲突（E0382）。
                    let shared = shared.clone();
                    let entity = entity.clone();
                    move |_, window, app| {
                        let mode = shared.right_mode.get();
                        let activating = !(mode == SidebarMode::Expanded
                            && shared.active_right.get() == panel_clone);
                        if !activating {
                            shared.right_mode.set(SidebarMode::Collapsed);
                        } else {
                            shared.open_right_panel(panel_clone, app);
                        }
                        entity.update(app, |this, cx| {
                            if activating {
                                this.open_right_panel(panel_clone, window, cx);
                            }
                            cx.notify();
                        });
                    }
                })
                .context_menu({
                    let shared = shared.clone();
                    let entity = entity.clone();
                    move |menu, _, _| {
                        let mut menu = menu.item(PopupMenuItem::label("右侧栏"));
                        menu = menu.item(PopupMenuItem::separator());
                        let shared = shared.clone();
                        let entity = entity.clone();
                        menu = menu.item(PopupMenuItem::new("隐藏侧边栏").on_click({
                            let shared = shared.clone();
                            let entity = entity.clone();
                            move |_, _, app| {
                                shared.right_mode.set(SidebarMode::Hidden);
                                entity.update(app, |_, cx| cx.notify());
                            }
                        }));
                        menu.item(PopupMenuItem::new("收起侧边栏").on_click({
                            let shared = shared.clone();
                            let entity = entity.clone();
                            move |_, _, app| {
                                shared.right_mode.set(SidebarMode::Collapsed);
                                entity.update(app, |_, cx| cx.notify());
                            }
                        }))
                    }
                });
            menu = menu.child(item);
        }

        let settings_shared = shared.clone();
        let settings_entity = entity.clone();
        let footer = div().h_flex().w_full().p_2().justify_center().child(
            Button::new("right-settings")
                .icon(Icon::new(IconName::Settings))
                .ghost()
                .on_click(move |_, window, app| {
                    let entity = settings_entity.clone();
                    entity.update(app, |this, cx| this.open_settings(window, cx));
                    settings_shared.quick_open.set(false);
                }),
        );

        Sidebar::new("right-activity")
            .side(Side::Right)
            .collapsible(SidebarCollapsible::Icon)
            .collapsed(true)
            .child(menu)
            .footer(footer)
            .into_any_element()
    }
}

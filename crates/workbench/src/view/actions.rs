//! 设置页、Quick Open 与 LSP 状态栏入口。
//!
//! 自 `view.rs` 拆分（行为不变）。

use super::status::{mock_status_chip, render_lsp_manager, sb_toggle, toggle_sidebar_hidden};
use super::*;
impl WorkbenchView {
    /// 打开设置（独立 OS 窗口）：窗口自带标题栏与系统窗口控制（最小化 / 最大化 /
    /// 关闭，`TitleBar::window_options()`）；存活检测经 `AnyWindowHandle::update`
    /// （窗口关闭后返回 Err），已打开则不重复创建。
    ///
    /// 页面自持两栏结构（分节与行由 `settings::registry` 驱动）；宿主只提供
    /// **宿主桥**（缓存管理 / 日志 / 类型简称映射，副作用在宿主侧）。
    /// T-1：把「标签栏最大宽度 / 样式」设置项装配到 DockSkin（启动一次 + 设置变更
    /// 经 `on_settings_changed` 重应用）。vendor `dock/tab_panel.rs` 渲染时从
    /// `SkinShared` 读取这两个值；未知样式字符串回退 `Tab`（设置页只给五档预设）。
    pub(super) fn apply_tab_bar_settings(skin: &Rc<DockSkin>, cx: &mut App) {
        let appearance = &settings::SettingsService::get(cx).appearance;
        skin.set_tab_bar_max_width(appearance.tab_bar_max_width, cx);
        let variant = match appearance.tab_bar_variant.as_str() {
            "outline" => TabVariant::Outline,
            "pill" => TabVariant::Pill,
            "segmented" => TabVariant::Segmented,
            "underline" => TabVariant::Underline,
            _ => TabVariant::Tab,
        };
        skin.set_tab_bar_variant(variant, cx);
    }

    pub(crate) fn open_settings(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        // 窗口存活检测：已打开则直接返回（用户自己切过去）。
        if let Some(handle) = self.settings_window {
            if handle.update(cx, |_, _, _| {}).is_ok() {
                return;
            }
            self.settings_window = None;
        }
        let shared_cache = self.shared.clone();
        let on_open_cache: Rc<dyn Fn(&mut Window, &mut App)> = Rc::new(move |window, app| {
            crate::components::cache_dialog::open_cache_dialog(window, app, &shared_cache);
        });
        // 「打开日志目录」/「查看日志」：设置层不依赖 engine / opener，这两个副作用由宿主实现。
        // 目录不存在（还没写过日志）就先建出来——"打开一个不存在的目录"是纯失败体验。
        let on_open_log_dir: Rc<dyn Fn(&mut Window, &mut App)> = Rc::new(move |_window, _app| {
            let dir = paths::log_dir();
            if let Err(e) = std::fs::create_dir_all(&dir) {
                eprintln!("[settings] 创建日志目录失败 {}: {e}", dir.display());
                return;
            }
            if let Err(e) = opener::open(&dir) {
                eprintln!("[settings] 打开日志目录失败 {}: {e}", dir.display());
            }
        });
        let on_open_log_view: Rc<dyn Fn(&mut Window, &mut App)> =
            Rc::new(move |window, app| crate::components::log_dialog::open_log_dialog(window, app));
        let on_open_type_short_map: Rc<dyn Fn(&mut Window, &mut App)> =
            Rc::new(move |window, app| {
                crate::components::type_short_map_dialog::open_type_short_map_dialog(window, app);
            });
        // 「打开配置文件」（设置页头部入口，Q3）：用系统默认 JSON 编辑器打开
        // `config/settings.json`；文件不存在（还没写过设置）就先建一个空 `{}`，
        // 否则"打开不存在的文件"是纯失败体验（与打开日志目录同一策略）。
        let on_open_settings_json: Rc<dyn Fn(&mut Window, &mut App)> =
            Rc::new(move |_window, _app| {
                let path = paths::config_dir().join("settings.json");
                if !path.exists() {
                    if let Err(e) = std::fs::write(&path, "{}") {
                        eprintln!("[settings] 创建 settings.json 失败 {}: {e}", path.display());
                        return;
                    }
                }
                if let Err(e) = opener::open(&path) {
                    eprintln!("[settings] 打开 settings.json 失败 {}: {e}", path.display());
                }
            });
        // T-1：设置落盘 → 重应用标签栏外观到 DockSkin（启动值在 `init_workspace`
        // 已应用一次；这里是运行时路径）。皮肤句柄在装配期已存字段。
        let dock_skin = self.dock_skin.clone();
        // 洞察偏好（`insight.*` 四项）变更也在这里推送：内存上限走 engine 连接级
        // 设置（即时生效），面板偏好（内存显示开关 / 工作集阈值）转发给面板实体。
        // 与 sink（`install_insight_memory_limit_sink`）双写幂等，这里兜底启动与面板重建。
        let shared_for_insight = self.shared.clone();
        // 【类型样式】结果集偏好（13 项）变更 → 遍历编辑器宿主推 `apply_result_style`
        // 即时生效（读当前 Settings 聚合，幂等；不随结果集 set_data 清掉）。
        let hosts_for_style = self.editor_hosts.clone();
        let on_settings_changed: Rc<dyn Fn(&mut App)> = Rc::new(move |app| {
            if let Some(skin) = &dock_skin {
                Self::apply_tab_bar_settings(skin, app);
            }
            crate::components::insight_actions::apply_insight_prefs(&shared_for_insight, app);
            for host in &hosts_for_style {
                host.update(app, |host, cx| host.apply_result_style(cx));
            }
        });
        let host = SettingsHost {
            on_open_cache,
            on_open_log_dir,
            on_open_log_view,
            on_open_type_short_map,
            on_open_settings_json,
            on_settings_changed,
        };
        // 独立窗口：`TitleBar::window_options()` 提供原生标题栏（拖拽 + 系统窗口控制），
        // 经 `cx.open_window` 挂到新窗口；窗口根由 `base::Root` 包一层（对话框层宿主）。
        // 定位交给平台默认（`window_bounds: None` → `display.default_bounds()`，通常居中）：
        // 显式 `Windowed(origin 0,0)` 会被级联偏移钉在屏幕左上角。仅给最小尺寸防过度挤压。
        let mut options = TitleBar::window_options();
        options.window_min_size = Some(gpui::Size::new(
            gpui::Pixels::from(workbench_shell::ui::SETTINGS_WINDOW_MIN_WIDTH),
            gpui::Pixels::from(workbench_shell::ui::SETTINGS_WINDOW_MIN_HEIGHT),
        ));
        let handle = cx
            .open_window(options, |window, cx| {
                let view = cx.new(|cx| SettingsPage::new(window, host, cx));
                cx.new(|cx| gpui_kit::base::Root::new(view, window, cx))
            })
            .expect("打开设置窗口");
        self.settings_window = Some(handle.into());
    }

    // ===== Quick Open（统一检索 / 命令面板）宿主装配 =====
    //
    // 浮层本体（输入 / 结果 / 键盘通道 / 防抖与回填）在 `quick_open::palette`；
    // 入口控件 / 懒创建 / 挂载 / 动作副作用落地在 `quick_open/host.rs`；
    // 这里只留开关（`Shared::quick_open` 是全应用一份的权威）。

    /// 打开 / 关闭 / 换档 Quick Open（标题栏入口与 `Ctrl+P` 共用默认档；`Ctrl+Shift+O` 跳转档）。
    ///
    /// 三态（审计 D11）：关着 → 打开；开着同档 → 关闭；开着**异档** → 换档不关闭
    /// （与 VSCode「同键位换档」肌肉记忆一致）。`mode` 只在**打开 / 换档边沿**落给浮层；
    /// 关闭由共享开关统一收口（浮层 Esc / 点遮罩 / 执行完）。
    /// `pub(crate)`：标题栏入口控件在 `quick_open/host.rs` 点击时回调。
    pub(crate) fn toggle_quick_open(&mut self, mode: Mode, cx: &mut Context<Self>) {
        let is_open = self.shared.quick_open.get();
        let current = is_open.then_some(self.quick_open_mode_pending);
        let intent = crate::quick_open::host::toggle_intent(is_open, current, mode);
        tracing::info!(?intent, mode = ?mode, "【探针】toggle_quick_open：Quick Open 浮层开关");
        match intent {
            crate::quick_open::host::ToggleIntent::Open
            | crate::quick_open::host::ToggleIntent::Switch => {
                self.shared.quick_open.set(true);
                self.quick_open_open_pending = true;
                self.quick_open_mode_pending = mode;
            }
            crate::quick_open::host::ToggleIntent::Close => {
                self.shared.quick_open.set(false);
            }
        }
        cx.notify();
    }

    /// 草稿箱文件属性弹窗（右键 → 属性；用户点名 2026-10-10：信息少，走弹窗不走 panel）。
    ///
    /// 读 `Shared::scratchpad_file_info`（草稿箱经端口写入）；宿主用 gpui-kit `Dialog`
    /// 包一层（内置 overlay / Esc / 遮罩点击关闭），内容只有 6 行键值（`render_scratchpad_file_info_pane`）。
    pub(super) fn render_scratchpad_file_info_popover(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let view = (*self.shared.scratchpad_file_info.borrow()).clone()?;
        let theme = cx.theme().clone();
        let shared = self.shared.clone();
        let close = move |_: &gpui_kit::ClickEvent, _: &mut gpui_kit::Window, app: &mut App| {
            *shared.scratchpad_file_info.borrow_mut() = None;
            shared.notify_host(app);
        };
        Some(
            Dialog::new(cx)
                .title(div().child(format!("属性 · {}", view.name)))
                .content({
                    let theme = theme.clone();
                    move |content: DialogContent, window: &mut Window, cx: &mut App| {
                        let _ = (window, cx);
                        content.child(render_scratchpad_file_info_pane(&view, &theme))
                    }
                })
                .button_props(DialogButtonProps::default().show_cancel(false))
                .on_close(close)
                .into_any_element(),
        )
    }

    // ===== LSP 管理器（状态栏入口 + 空闲回收） =====

    /// 【LSP】空闲回收 + 状态栏指示刷新（1 s 一拍；句柄必须被持有，drop 即停）
    ///
    /// 两件事共用一个节拍：① `reap_if_idle`——**关掉最后一个需要服务的面板 → 停 server**
    /// （只停服务、不设拦住原因，下次需要会自动重启）；② 端口状态版本变了就 `notify`，
    /// 让状态栏指示（图标 / 文案）跟上（启动 / 停止 / 就绪 / 崩溃）。
    pub(super) fn spawn_lsp_reaper(&self, cx: &mut Context<Self>) -> Option<Task<()>> {
        self.lsp.as_ref()?;
        let weak = cx.entity().downgrade();
        let executor = cx.background_executor().clone();
        Some(cx.spawn(async move |_this, cx| {
            loop {
                executor.timer(std::time::Duration::from_millis(1000)).await;
                let keep = weak
                    .update(cx, |view, cx| {
                        // 复用自持端口句柄（与 `view.lsp` 同一个 Rc；先 clone 开，
                        // 避开 `view` 的借用与 `cx.notify` 的写冲突）
                        let Some(lsp) = view.lsp.clone() else {
                            return false;
                        };
                        lsp.reap_if_idle();
                        let epoch = lsp.epoch();
                        if epoch != view.last_lsp_epoch {
                            view.last_lsp_epoch = epoch;
                            cx.notify();
                        }
                        true
                    })
                    .unwrap_or(false);
                if !keep {
                    return;
                }
            }
        }))
    }

    /// 【LSP】立即把状态版本对齐到端口最新值（手动启停后立刻重绘，不等 1 s 泵）
    pub(super) fn sync_lsp_epoch(&mut self) {
        if let Some(lsp) = self.lsp.as_ref() {
            self.last_lsp_epoch = lsp.epoch();
        }
    }

    /// 【LSP】状态栏「语言服务」指示 + 管理器浮层（点击弹：启动 / 停止）
    ///
    /// 未接端口（测试 / 未启用）→ 空元素（不摆这一项）。触发元素必须是 `Button`
    /// （`Popover::trigger` 要求 `Selectable`，元素侧唯一实现者是 `Button`）；用 `.xsmall()`
    /// + 自定义变体对齐状态栏本色（`primary` / `primary_foreground`），避开把 h_6 的按钮
    /// 挤进 `text_xs` 的状态栏里把整条撑高。
    pub(super) fn lsp_status_item(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(lsp) = self.lsp.as_ref() else {
            return div().into_any_element();
        };
        let (icon, label) =
            lsp_status_chip(lsp.is_running(), lsp.is_initialized(), lsp.block().as_ref());
        let (fg, hover) = {
            let colors = &cx.theme().colors;
            (colors.primary_foreground, colors.primary_active)
        };
        let entity = cx.entity();
        Popover::new("status-lsp-popover")
            // 状态栏在最下：向左上展开，避开被窗口下缘截断
            .anchor(Anchor::BottomRight)
            .offset(ui::STATUS_LSP_OFFSET)
            .trigger(
                Button::new("status-lsp")
                    .custom(
                        ButtonCustomVariant::new(cx)
                            .color(cx.theme().transparent)
                            .foreground(fg)
                            .hover(hover),
                    )
                    .xsmall()
                    .icon(icon)
                    .label(label)
                    .tooltip("语言服务（LSP）"),
            )
            .content(move |_state, _window, cx| render_lsp_manager(&entity, cx))
            .into_any_element()
    }

    // ===== 状态栏 =====

    pub(super) fn render_status_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        // 【LSP】语言服务指示 + 管理器浮层（未接端口 = 空元素）——
        // 必须在 `cx.theme()` 之前算：那一步把 cx 借出去了（gpui-kit-dev 的借用约定）。
        let lsp_item = self.lsp_status_item(cx);
        // M7：Mock 任务在跑时给状态栏一个指示 + 取消入口（读面板实体自己的状态，不另存一份）——
        // 必须在 `cx.theme()` 之前算：那一步把 cx 借出去了（gpui-kit-dev 的借用约定）。
        let mock_chip = self
            .shared
            .mock_panel
            .borrow()
            .clone()
            .and_then(|weak| weak.upgrade())
            .map(|panel| mock_status_chip(panel, cx));
        let theme = cx.theme();
        let entity = cx.entity();
        let shared = self.shared.clone();
        let selected = self
            .shared
            .selected_connection()
            .map(|c| c.name)
            .unwrap_or_else(|| "未选择连接".to_string());
        let active_label = self.shared.active_left.get().label();
        let left_hidden = self.shared.left_mode.get() == SidebarMode::Hidden;
        let right_hidden = self.shared.right_mode.get() == SidebarMode::Hidden;

        // 左侧独立开关：完全隐藏 / 恢复（恢复时还原隐藏前模式）。
        // 自绘（对齐设计稿 .sb-btn）：显式前景色 + hover 高亮，不依赖 Button 变体样式。
        let left_shared = shared.clone();
        let left_entity = entity.clone();
        let left_toggle = sb_toggle(
            "toggle-left-sidebar",
            IconName::PanelLeftClose,
            if left_hidden {
                "» 恢复"
            } else {
                "« 完全隐藏"
            },
            theme.colors.primary_foreground,
            theme.colors.primary_active,
            move |app| toggle_sidebar_hidden(&left_shared, &left_entity, true, app),
        );

        // 右侧独立开关（镜像）。
        let right_shared = shared.clone();
        let right_entity = entity.clone();
        let right_toggle = sb_toggle(
            "toggle-right-sidebar",
            IconName::PanelRightClose,
            if right_hidden {
                "« 恢复"
            } else {
                "完全隐藏 »"
            },
            theme.colors.primary_foreground,
            theme.colors.primary_active,
            move |app| toggle_sidebar_hidden(&right_shared, &right_entity, false, app),
        );

        // 布局锁定开关。
        let layout_locked = self.shared.layout_locked.get();
        let lock_shared = shared.clone();
        let lock_entity = entity.clone();
        let lock_toggle = sb_toggle(
            "toggle-layout-lock",
            IconName::Settings,
            if layout_locked {
                "解锁布局"
            } else {
                "锁定布局"
            },
            theme.colors.primary_foreground,
            theme.colors.primary_active,
            move |app| {
                let old_state = lock_shared.layout_locked.get();
                let new_state = !old_state;
                lock_shared.layout_locked.set(new_state);
                // 同步到设置页面
                settings::SettingsService::set_layout_locked(new_state, app);
                lock_entity.update(app, |_, cx| cx.notify());
            },
        );

        // 状态栏背景品牌珊瑚色（theme.colors.primary 派生，见 theme-design §5.4），文字取配对前景色。
        StatusBar::new()
            .bg(theme.colors.primary)
            .text_color(theme.colors.primary_foreground)
            .left(
                div()
                    .h_flex()
                    .items_center()
                    .gap_2()
                    .text_xs()
                    .child(left_toggle)
                    .child(div().child(active_label)),
            )
            .right(
                div()
                    .h_flex()
                    .items_center()
                    .gap_2()
                    .text_xs()
                    // 状态栏去杂乱（需求）：文档级上下文已迁到编辑器自身状态栏，
                    // 全局栏只留应用级信息——Mock 任务 / 选中连接 / LSP / 右侧开关，
                    // 去掉静态的「DuckDB 就绪」「UTF-8」。
                    .children(mock_chip)
                    .child(div().child(format!("连接：{selected}")))
                    .child(lsp_item)
                    .child(notification::notification_bell(cx))
                    .child(lock_toggle)
                    .child(right_toggle),
            )
    }
}

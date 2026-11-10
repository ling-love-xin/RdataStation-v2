//! `WorkbenchView` 的 `Render` 实现（布局权威同步点）。
//!
//! 自 `view.rs` 拆分（行为不变）。

use super::*;
impl Render for WorkbenchView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.area.is_none() {
            self.init_workspace(window, cx);
        }
        // 搜索结果标签页（「搜索: query」，VSCode 全局搜索形态）：草稿箱搜索完成
        // （`Shared::scratchpad_search` 有值）且面板未建 → `cx.defer_in` 建面板
        // （事件路径可改 dock；render 路径本身只读）。面板已建（含死弱引用升级成功）
        // 则不重复入队——新搜索结果只更新全局状态，面板每帧读到最新内容。
        // 用户 tab ✕ 关闭过标签页（`search_results_dismissed`）则不自动重建——
        // 关标签保留结果，直到再次搜索（`show_scratchpad_search(Some)` 清标志）。
        let search_matches = self
            .shared
            .search_results_panel
            .borrow()
            .as_ref()
            .and_then(|weak| weak.upgrade())
            .map(|panel| {
                let s = self.shared.scratchpad_search.borrow();
                panel.read(cx).query() == s.as_ref().map(|v| v.query.clone())
            })
            .unwrap_or(false);
        if self.shared.scratchpad_search.borrow().is_some()
            && !*self.shared.search_results_dismissed.borrow()
            && !search_matches
        {
            cx.defer_in(window, |this, window, cx| {
                this.ensure_search_results_panel(window, cx);
            });
        }
        // 元数据搜索标签页（P0，VSCode「搜索: query」形态，搜连接级元数据缓存）：
        // 面板自持泵把结果写进 `Shared::metadata_search`（有值）且面板未建 →
        // `cx.defer_in` 建面板（事件路径可改 dock）。用户 tab ✕ 关闭过则不自动重建——
        // 关标签保留结果，直到再次搜索（`publish_metadata_search(Some)` 清标志）。
        let md_search_matches = self
            .shared
            .metadata_search_panel
            .borrow()
            .as_ref()
            .and_then(|weak| weak.upgrade())
            .map(|panel| {
                let s = self.shared.metadata_search.borrow();
                panel.read(cx).query() == s.as_ref().map(|v| v.query.clone())
            })
            .unwrap_or(false);
        if self.shared.metadata_search.borrow().is_some()
            && !*self.shared.metadata_search_dismissed.borrow()
            && !md_search_matches
        {
            cx.defer_in(window, |this, window, cx| {
                this.ensure_metadata_search_panel(window, cx);
            });
        }
        // 洞察中间工作页（D63）：入口（导航树 / 结果列表 / SQL 编辑器）登记了待打开
        // 目标 → `cx.defer_in` 建 / 激活中央洞察 tab（事件路径可改 dock；render 纯读）。
        // 面板已建且正在渲染同一目标时，入口重复触发只聚焦不重发（ensure 消费后为空）。
        if self.shared.pending_insight_target.borrow().is_some() {
            cx.defer_in(window, |this, window, cx| {
                this.ensure_insight_workspace_panel(window, cx);
            });
        }
        // T18：右栏集合「对比」请求 → 中间页切到对比形态（勾选该表 + 激活中央 tab）。
        if self.shared.pending_insight_compare.borrow().is_some() {
            cx.defer_in(window, |this, window, cx| {
                this.consume_insight_compare(window, cx);
            });
        }
        // M1：项目输入实体懒创建（选择器搜索 / 新建 / 删除确认）。
        if self.project_inputs.is_none() {
            self.project_inputs = Some(project::ui::ProjectInputs::new(window, cx));
        }
        // 三模式权威同步点：Shared 状态 → Dock。
        self.apply_left_mode(window, cx);
        self.apply_right_mode(window, cx);
        // 布局锁定同步：Settings 状态 → Shared 状态 → Dock。
        // 从设置页面读取布局锁定状态，然后同步到 Shared 结构体。
        // 暂时注释掉，看看是否能解决分割线不能拖动的问题。
        // let settings_locked = settings::SettingsService::layout_locked(cx);
        // let shared_locked = self.shared.layout_locked.get();
        // if settings_locked != shared_locked {
        //     eprintln!("[DEBUG] 布局锁定同步：Settings.locked = {}, Shared.locked = {}, 正在同步", settings_locked, shared_locked);
        //     self.shared.layout_locked.set(settings_locked);
        // }
        // if let Some(area) = self.area.clone() {
        //     let locked = self.shared.layout_locked.get();
        //     area.update(cx, |area, cx| {
        //         area.set_locked(locked, window, cx);
        //     });
        // }
        // 侧边栏宽度同步：设置值 → Dock。
        // 只在设置页面的值发生变化时，才同步侧边栏宽度到 Dock，避免覆盖用户拖动分割线后的大小。
        //
        // 单位口径统一（2026-11-01）：Settings 里的宽度是**基准字号下的 px**（用户看到的就是这个数），
        // 应用时须按当前字号放大后再交给 Dock。过去这里直接写 `px(left_width)`，把上面
        // `apply_left_mode` 刚写进去的 `font_size × ui::LEFT_DOCK_WIDTH` 结果覆盖掉——用户只要
        // 配过宽度，字号缩放就在这一行被打断。缺基准值时用默认值兜底，避免除 0。
        let left_width = settings::SettingsService::layout_left_sidebar_width(cx) as f32;
        let right_width = settings::SettingsService::layout_right_sidebar_width(cx) as f32;
        let prev_left_width = self.shared.prev_left_sidebar_width.get();
        let prev_right_width = self.shared.prev_right_sidebar_width.get();
        if left_width != prev_left_width || right_width != prev_right_width {
            if let Some(area) = self.area.clone() {
                area.update(cx, |area, cx| {
                    let font_size = cx.theme().font_size;
                    // `font_size` 与 `px(BASE_FONT_SIZE)` 同为 `Pixels`：`Pixels / Pixels` 才是无单位比值
                    // （`Pixels / f32` 会得到 `Pixels`，再乘 px 宽度就变成面积量纲，编译期即报错）。
                    let scale: f32 = if ui::BASE_FONT_SIZE > 0.0 {
                        font_size / px(ui::BASE_FONT_SIZE)
                    } else {
                        1.0
                    };
                    area.set_dock_size(DockPlacement::Left, px(left_width * scale), window, cx);
                    area.set_dock_size(DockPlacement::Right, px(right_width * scale), window, cx);
                });
            }
            self.shared.prev_left_sidebar_width.set(left_width);
            self.shared.prev_right_sidebar_width.set(right_width);
        }
        // 【2026-10-27】结果区从全局底部 dock 拆回编辑器宿主内嵌（DBeaver 口径：
        // 结果集跟编辑器 tab 走，不再挂全局 dock）。结果区有无内容由编辑器宿主自己
        // render 判断（v_resizable 分栏，无内容时不占位）。旧 `result_dock.rs` 薄壳
        // 与 `sync_result_dock` 已整体移除（孤儿实体，见 2026-10-28 清理）。
        // 真面板化：版本 / 修复 / 回收站 / 标签对话框与导航「定位」请求原在
        // `SidebarPanel::render` 消费（外壳已不再渲染），提升到宿主 render——
        // render 仍是权威同步点，副作用不放到事件路径；动作仍经 `sidebar` 实体转发。
        if let Some(sidebar) = self.sidebar.clone() {
            sidebar.update(cx, |panel, cx| {
                panel.ensure_version_dialog(window, cx);
                panel.ensure_repair_dialog(window, cx);
                panel.ensure_trash_dialog(window, cx);
                panel.ensure_tag_dialog(window, cx);
                panel.consume_reveal(cx);
            });
        }
        // 连接对话框项目下拉的动作项 → 复用项目管理的入口：
        // 「＋ 新增项目」开新建对话框；「打开现有目录…」开目录选择对话框。
        // 两者都会切换项目；有未保存草稿时先走确认，**确认后直接推进到目标对话框**（#4）。
        // 决策与“只消费一次”收在 `Shared::take_project_action_request`（可单测，#9）。
        if let Some(request) = self.shared.take_project_action_request() {
            let host = self.project_host().clone();
            if let Some(inputs) = self.project_inputs.clone() {
                match request {
                    ProjectActionRequest::CreateProject => {
                        project::ui::request_create_project(&host, &inputs, window, cx);
                    }
                    ProjectActionRequest::OpenFolder => {
                        project::ui::request_open_folder(&host, &inputs, window, cx);
                    }
                }
            }
        }
        // M5 草稿箱 / M6 资产库：双击 / Enter / 右键「打开」→ 中央编辑器
        //（同路径已打开只激活，不重读；M6 的本体带编辑器只读）。
        // 消费点在宿主 render（文档与 Dock 面板属宿主状态）；失败走通知栏。
        if let Some(request) = self.shared.take_open_in_editor() {
            if let Err(e) = self.open_in_editor_with(
                request.path,
                request.read_only,
                request.reveal,
                window,
                cx,
            ) {
                self.shared.say_error(format!("打开文件失败: {e}"), cx);
            }
        }
        // B11：导航的「打开查询」请求（在 SQL 编辑器中打开 / 查看数据 / 生成 SQL / 拖拽）。
        // 入队方拿不到 `Window`，消费点在宿主 render（与上一块同一口径）。
        if let Some(request) = self.shared.take_query_request() {
            self.open_query_document(request, window, cx);
        }

        let area = self.area.clone().expect("workspace initialized");
        // edition 2024：`.then(|| ...)` 闭包会同时独占 `cx`/`self`，改为显式 if（也更符合
        // 编码指南「分支代表不同 interface 时用普通控制流」）。
        let left_bar = if self.shared.left_mode.get() != SidebarMode::Hidden {
            Some(self.render_left_activity_bar(cx))
        } else {
            None
        };
        let right_bar = if self.shared.right_mode.get() != SidebarMode::Hidden {
            Some(self.render_right_activity_bar(cx))
        } else {
            None
        };
        let quick_open = self.render_quick_open(cx);
        let scratchpad_info = self.render_scratchpad_file_info_popover(window, cx);

        // M1：项目选择器已是中央 tab 组常驻面板（`ProjectPickerPanel::visible` 随
        // 项目开关），DockArea 不再分「有项目 / 无项目」两套挂法——无项目时左右
        // Dock 照常渲染，Mock 面板「纯生成不受影响」（mock-architecture I8）。
        let mut middle = div().h_flex().items_stretch().flex_1().min_h_0();
        if let Some(bar) = left_bar {
            middle = middle.child(bar);
        }
        middle = middle.child(area);
        if let Some(bar) = right_bar {
            middle = middle.child(bar);
        }

        let mut root = div()
            .v_flex()
            .size_full()
            .min_h_0()
            .relative()
            .key_context("workbench")
            .on_action({
                let entity = cx.entity();
                move |_: &ToggleQuickOpen, _window, cx| {
                    entity.update(cx, |this, cx| this.toggle_quick_open(Mode::Default, cx));
                }
            })
            .on_action({
                let entity = cx.entity();
                move |_: &ToggleQuickGoto, _window, cx| {
                    entity.update(cx, |this, cx| this.toggle_quick_open(Mode::Goto, cx));
                }
            })
            // M4：Ctrl+F 聚焦数据源导航搜索（先切到数据源面板并展开左侧 Dock）。
            .on_action({
                let sidebar = self.sidebar.clone();
                let entity = cx.entity();
                move |_: &FocusNavSearch, window, cx| {
                    if let Some(sidebar) = &sidebar {
                        sidebar.update(cx, |panel, cx| panel.focus_nav_search(window, cx));
                    }
                    entity.update(cx, |this, cx| {
                        this.shared.active_left.set(LeftPanel::Database);
                        if this.shared.left_mode.get() == SidebarMode::Hidden {
                            this.shared.left_mode.set(SidebarMode::Expanded);
                        }
                        cx.notify();
                    });
                }
            })
            .on_action({
                let entity = cx.entity();
                move |_: &HideSidebars, _window, cx| {
                    entity.update(cx, |this, cx| {
                        let s = &this.shared;
                        if s.left_mode.get() != SidebarMode::Hidden {
                            s.left_mode_before_hidden.set(s.left_mode.get());
                            s.left_mode.set(SidebarMode::Hidden);
                        }
                        if s.right_mode.get() != SidebarMode::Hidden {
                            s.right_mode_before_hidden.set(s.right_mode.get());
                            s.right_mode.set(SidebarMode::Hidden);
                        }
                        cx.notify();
                    });
                }
            })
            .on_action({
                let entity = cx.entity();
                move |_: &RestoreSidebars, _window, cx| {
                    entity.update(cx, |this, cx| {
                        let s = &this.shared;
                        if s.left_mode.get() == SidebarMode::Hidden {
                            s.left_mode
                                .set(restore_snapshot(s.left_mode_before_hidden.get()));
                        }
                        if s.right_mode.get() == SidebarMode::Hidden {
                            s.right_mode
                                .set(restore_snapshot(s.right_mode_before_hidden.get()));
                        }
                        cx.notify();
                    });
                }
            })
            .on_action({
                let entity = cx.entity();
                move |_: &OpenSettings, window, cx| {
                    tracing::info!("【探针】OpenSettings：打开设置窗口（Ctrl+,）");
                    entity.update(cx, |this, cx| this.open_settings(window, cx));
                }
            })
            // 主题切换（`Ctrl+Shift+L`，绑在 workbench context）：显式模式两态切换；
            // 跟随系统时按系统当前外观反切到显式对侧。处理在 SettingsService（设置层）。
            .on_action({
                move |_: &ToggleThemeMode, window, cx| {
                    SettingsService::toggle_theme_mode(Some(window), cx);
                }
            })
            // M1：切换项目（= 关闭当前 + 回选择器）。
            .on_action({
                let entity = cx.entity();
                move |_: &SwitchProject, window, cx| {
                    entity.update(cx, |this, cx| {
                        tracing::info!(
                            "【探针】SwitchProject：切换项目动作触发（关闭当前回选择器）"
                        );
                        let host = this.project_host().clone();
                        project::ui::request_close(&host, window, cx);
                    });
                }
            })
            // M1：关闭项目。
            .on_action({
                let entity = cx.entity();
                move |_: &CloseProject, window, cx| {
                    entity.update(cx, |this, cx| {
                        tracing::info!("【探针】CloseProject：关闭项目动作触发");
                        let host = this.project_host().clone();
                        project::ui::request_close(&host, window, cx);
                    });
                }
            })
            // A10：关闭当前编辑器文档（键位绑在编辑器面板的 `editor` context 上，
            // 但在这里执行：面板不在自己的 `update` 里，Dock 移除它时才不会重入）。
            .on_action({
                let entity = cx.entity();
                move |_: &editor::commands::CloseDocument, window, cx| {
                    entity.update(cx, |this, cx| this.close_active_editor(window, cx));
                }
            })
            // A9：另存为 / 打开文件——同样在这里执行（系统文件对话框是宿主依赖，
            // 且另存为要落到“当前是哪份文档”上）。
            .on_action({
                let entity = cx.entity();
                move |_: &editor::commands::SaveDocumentAs, window, cx| {
                    entity.update(cx, |this, cx| this.save_active_editor_as(window, cx));
                }
            })
            .on_action({
                let entity = cx.entity();
                move |_: &editor::commands::OpenDocument, window, cx| {
                    entity.update(cx, |this, cx| this.open_file_via_dialog(window, cx));
                }
            })
            .child(self.render_title_bar(window, cx))
            .child(middle)
            .child(self.render_status_bar(cx));

        if let Some(qo) = quick_open {
            root = root.child(qo);
        }
        if let Some(info) = scratchpad_info {
            root = root.child(info);
        }
        // M1：项目设置浮层（菜单 / 对话框已改由 Popover 与语义 Dialog 承载，不在此渲染）。
        if self.shared.project.borrow().is_some() {
            let host = self.project_host().clone();
            let inputs = self
                .project_inputs
                .clone()
                .expect("project inputs initialized");
            if let Some(settings) = project::ui::render_settings(&host, &inputs, cx) {
                root = root.child(settings);
            }
        }
        // 对话框层：gpui-kit 0.7.1 的 Root 已自动挂载 dialog 层（WindowState::dialog_layer），
        // 连接对话框与项目对话框不再需要手动渲染。
        // 全局通知系统（NotificationList）：懒初始化并挂载到渲染树。
        // 用 absolute 定位，不影响主布局。
        let notification_list = notification::ensure_notifications(window, cx);
        root.child(div().absolute().size_full().child(notification_list))
    }
}

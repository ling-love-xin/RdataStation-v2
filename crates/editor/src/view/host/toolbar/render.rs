//! 工具栏主渲染（render_toolbar / render_toolbar_group / render_toolbar_item / 基础按钮）
//!
//! 自 host/toolbar.rs 拆分（2026-10-29 Step 8）：按能力域切分 `impl EditorHostPanel` 方法块。
//! 跨域互调经 `pub(crate)` 方法；字段在 mod.rs 统一 `pub(crate)`。

use super::super::*;
use gpui_kit::assets::IconName;
use gpui_kit::component::toolbar::Toolbar;
use crate::channel::ExecChannel;
use crate::execution::ExecMenuKind;
use crate::model::EditorMode;
use crate::ui;

impl EditorHostPanel {

    /// 工具栏（原型 §2.2 + T2c-5 分组自定义）：按其分层只放**今天真有动作**的控件
    ///
    /// - 最左：**模式指示器**（文本 / SQL / 分析，点击切换）——固定项，不参与
    ///   分组 / 排序 / 可见性（原型 ⑯）。
    /// - 左区：按 [`crate::toolbar`] 的分组模型渲染（组 = 分隔符界定的连续段；折叠组
    ///   显示为「首组件 + ▾」，▾ 收纳组内其余可见成员；展开组逐可见项渲染）。
    ///   **只在 SQL 模式出现**：文本模式按能力表不通信，分析模式的执行属笔记级动作
    ///   （1c 随单元落地）。
    /// - 右对齐簇（执行位置 + 连接）：T2c-5 里固定在最右，不参与分组移动
    ///   （右对齐语义稳定；T2c-6 随会话持久化时再放开）。
    /// - 最右：**`···` 定制按钮**——打开「工具栏自定义」面板（可见性 / 折叠 / 移动 /
    ///   竖线分隔符 / 重置）。
    pub(crate) fn render_toolbar(&self, mode: EditorMode, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let border = cx.theme().colors.border;
        let entity = cx.entity();
        // 模式指示器的 dropdown_menu 闭包会 move 走 `entity`，工具栏按钮也要用宿主：
        // 先克隆一份，避免「move 之后再 borrow」（E0382）
        let toolbar_entity = entity.clone();
        let current_channel = self.channel();

        // 【组件统一化】外壳迁移到 gpui-kit Toolbar（获得：左右方向键焦点漫游 +
        // 档位统一）。Small 档是 min_h_8(32px)，editor 按原型 2.25rem 定高覆盖；
        // Small 档 p_1=4px 内距，恢复原型水平 8px（px_2 覆盖）。底部边框随外层
        // 滚动容器走（2026-10-28），避免横向滚动时边框跟着内容一起滚。
        tracing::info!("【Toolbar埋点】使用 gpui-kit Toolbar 组件渲染编辑器工具栏");
        let mut toolbar = Toolbar::new("editor-toolbar")
            .small()
            .h(rems(ui::EDITOR_TOOLBAR_HEIGHT))
            .px_2()
            .content(
                Button::new("editor-mode-indicator")
                    .ghost()
                    .small()
                    // 测试按选择器断言“这个模式该有哪些控件”：模式与工具栏分层不是装饰
                    .debug_selector(|| "editor-mode-indicator".to_string())
                    // 原型 ⑯：图标 + 模式名 + ▾（图标由 toolbar 层的纯函数给出）
                    .icon(crate::toolbar::mode_icon(mode))
                    .label(format!("{} ▾", mode.label()))
                    .dropdown_menu(move |menu, _window, _cx| {
                        let mut menu = menu;
                        for candidate in EditorMode::ALL {
                            let is_current = candidate == mode;
                            let entity = entity.clone();
                            menu = menu.item(
                                PopupMenuItem::new(candidate.label())
                                    // 当前模式打勾而不是隐藏：三个模式总在同一处可切
                                    .checked(is_current)
                                    .on_click(move |_, window, app| {
                                        entity.update(app, |panel, cx| {
                                            panel.request_mode_switch(candidate, window, cx);
                                        });
                                    }),
                            );
                        }
                        menu
                    }),
            );

        // 【书签】工具栏入口（三模式通用：模式指示器后固定位置，不随组移动）。
        // 打开书签管理器浮层（列表 + 跳转 / 编辑说明 / 删除）；书签是文档级标记，
        // 与执行通道等 SQL 专属控件分开——任何模式都该能看「这个文件标了什么」。
        toolbar = toolbar.child(
            Button::new("editor-bookmarks")
                .ghost()
                .small()
                .icon(IconName::BookMarked)
                .tooltip("书签")
                .debug_selector(|| "editor-bookmarks".to_string())
                .on_click({
                    let toolbar_entity = toolbar_entity.clone();
                    move |_, window, cx| {
                        toolbar_entity.update(cx, |panel, cx| {
                            panel.open_bookmark_manager(window, cx);
                        });
                    }
                }),
        );

        // 【核对】工具栏形态以能力表为唯一分叉（不再按 mode 硬编码）：Full = SQL 组列表 +
        // 执行位置 / 连接；Minimal = 环境 + 脚本运行；Notebook = 笔记级动作 1c 留白。
        match crate::model::Capabilities::for_mode(mode).toolbar {
            crate::model::ToolbarStyle::Full => {
            // 左区：组列表（T2c-5）。只有 SQL 模式的执行是“对当前文档执行”：
            // 文本模式按能力表不通信，分析模式的执行是**笔记级**动作（1c 落地）。
            // 函数手册（#54-C5）：SQL 模式独有入口（通用/分析模式的能力表里没有它）。
            toolbar = toolbar.child(
                Button::new("editor-function-manual")
                    .ghost()
                    .small()
                    .icon(IconName::BookMarked)
                    .tooltip("函数手册")
                    .debug_selector(|| "editor-function-manual".to_string())
                    .on_click({
                        let toolbar_entity = toolbar_entity.clone();
                        move |_, window, cx| {
                            toolbar_entity.update(cx, |panel, cx| {
                                _ = panel.open_function_manual(window, cx);
                            });
                        }
                    }),
            );
            let groups = self.toolbar.groups();
            for group in &groups {
                toolbar = toolbar.content(self.render_toolbar_group(group, cx));
            }
            // 通道级 + 连接（右对齐）：原型 §2.2 两栏——“执行位置”在“连接”左边；
            // T2c-5 固定不动（右对齐语义稳定）。
            toolbar = toolbar.content(
                div()
                    .ml_auto()
                    .h_flex()
                    .items_center()
                    .gap_1()
                    .when(current_channel == ExecChannel::Federated, |row| {
                        row.child(self.render_sources_picker(cx))
                    })
                    // 通道级 + 连接（原型 §2.2 通道级+连接层，工具栏右区）：
                    // 三固定区：执行通道 pill / 连接名称（运行点 + 短码·名称，只列实时在线、
                    // 默认跟随当前连接）/ schema@db——各自固定宽 + 截断，不随内容推挤布局
                    // （2026-10-27；连接定位自状态栏迁此，状态栏左段只留方言/事务等）
                    .children(render_channel_control(self, &cx.entity()))
                    .children(render_conn_slots(self, &cx.entity(), cx.theme()))
                    // 2026-10-27：roLock 在工具栏；折叠开关已移设置页（render 权威同步点）
                    .child(self.render_ro_lock_button(cx)),
            );
            // 最右：`···` 定制面板（布局自定义；低频动作已全部提升常驻，见原型 §2.2 修订）
            toolbar = toolbar.content(self.render_toolbar_customize(cx));
            }
            crate::model::ToolbarStyle::Minimal => {
            // 【2b】通用模式：环境选择器 + 运行按钮（无连接档：不摆执行位置 / 连接面包屑）
            // Markdown 文档额外有「预览 / 源码」切换（渲染态是只读视图）。
            toolbar = toolbar.content(
                div()
                    .ml_auto()
                    .h_flex()
                    .items_center()
                    .gap_1()
                    .when(self.is_markdown_document(), |row| {
                        row.child(self.render_markdown_preview_toggle(cx))
                    })
                    .child(self.render_env_picker(cx))
                    .child(self.render_run_script_button(cx))
                    // 2026-10-27：roLock 也挂通用模式工具栏；折叠开关已移设置页
                    .child(self.render_ro_lock_button(cx)),
            );
            }
            // Notebook：笔记级工具栏（会话 ▾ / 全部运行 / 重启）——1c 落地。
            // 与 SQL 组模型无关：会话绑定（连接）是笔记级事实，执行是顺序跑全部单元。
            crate::model::ToolbarStyle::Notebook => {
                toolbar = toolbar.content(
                    div()
                        .ml_auto()
                        .h_flex()
                        .items_center()
                        .gap_1()
                        .child(self.render_notebook_session_button(cx))
                        .child(self.render_notebook_run_all_button(cx))
                        .child(self.render_notebook_restart_button(cx)),
                );
            }
        }
        // 【工具栏溢出滚动】（2026-10-28）：gpui-kit Toolbar 组件 `flex_shrink_0` 不收缩，
        // 面板被右侧 Dock 挤窄时按钮溢出会被遮挡。外包横向滚动视口 + 两端悬浮箭头按钮：
        // 溢出判定读 ScrollHandle 上一帧 `max_offset`（负值 = 内容确实溢出），箭头只在
        // 对应方向还有内容时出现——右侧 Dock 挤压时右侧按钮可滚动查看，不再被盖住。
        // 悬浮箭头是独立按钮（标准 Button），不手搓控件；滚动是无动画的 offset 平移。
        // 视口用 gpui 原生横向滚动（`.id()` 后挂 `overflow_x_scroll` + `track_scroll`）而非
        // kit Scrollable：后者会把内层元素 id 换成 `(id, "content")`，工具栏内 `editor-conn-name`
        // 等元素的 id 会脱离测试观测（debug_bounds 找不到）；原生滚动不替换 id，行为与 TabBar 同构。
        let scroll_handle: ScrollHandle = window
            .use_keyed_state("editor-toolbar-scroll", cx, |_, _| ScrollHandle::default())
            .read(cx)
            .clone();
        let offset_x = scroll_handle.offset().x;
        let max_off = scroll_handle.max_offset().x;
        let has_overflow = max_off < Pixels::from(0.);
        let show_left = has_overflow && offset_x < Pixels::from(0.);
        let show_right = has_overflow && offset_x > max_off;
        // 单步约 5 个图标按钮宽（toolbar small 档按钮 ≈32px）。
        let arrow_step = Pixels::from(160.);
        let theme = cx.theme();
        let arrow_bg = theme.colors.sidebar;

        let mut bar = div()
            .relative()
            .h_flex()
            .w_full()
            .min_w_0()
            .h(rems(ui::EDITOR_TOOLBAR_HEIGHT))
            .border_b(ui::HAIRLINE)
            .border_color(border)
            .child(
                div()
                    .id("editor-toolbar-viewport")
                    .flex_1()
                    .min_w_0()
                    .overflow_x_scroll()
                    .lock_scroll_axis()
                    .track_scroll(&scroll_handle)
                    .child(toolbar),
            );
        if show_left {
            let handle = scroll_handle.clone();
            bar = bar.child(
                div()
                    .absolute()
                    .left(Pixels::from(0.))
                    .top(Pixels::from(2.))
                    .bottom(Pixels::from(2.))
                    .v_flex()
                    .items_center()
                    .justify_center()
                    .child(
                        Button::new("editor-toolbar-scroll-left")
                            .ghost()
                            .small()
                            .icon(IconName::ChevronLeft)
                            .tooltip("向左滚动工具栏")
                            .bg(arrow_bg.opacity(0.9))
                            .on_click(move |_, _, _| {
                                let cur = handle.offset().x;
                                // 回看左侧：offset 增加（趋近 0），最左即 0。
                                handle.set_offset(point((cur + arrow_step).min(Pixels::from(0.)), Pixels::from(0.)));
                            }),
                    ),
            );
        }
        if show_right {
            let handle = scroll_handle.clone();
            bar = bar.child(
                div()
                    .absolute()
                    .right(Pixels::from(0.))
                    .top(Pixels::from(2.))
                    .bottom(Pixels::from(2.))
                    .v_flex()
                    .items_center()
                    .justify_center()
                    .child(
                        Button::new("editor-toolbar-scroll-right")
                            .ghost()
                            .small()
                            .icon(IconName::ChevronRight)
                            .tooltip("向右滚动工具栏")
                            .bg(arrow_bg.opacity(0.9))
                            .on_click(move |_, _, _| {
                                let cur = handle.offset().x;
                                // 看右侧：offset 减小（更负），最右 clamp 到 max_offset。
                                handle.set_offset(point((cur - arrow_step).max(max_off), Pixels::from(0.)));
                            }),
                    ),
            );
        }
        bar
    }

    /// 【2026-10-27】用户只读开关（工具栏 roLock）：`toggled` = 只读
    ///
    /// 点击在 只读/编辑 间翻转（窗口级合成只读，见 [`Self::read_only_flags`]；
    /// 内核 readonly 一并同步）。SQL 与通用模式工具栏都挂。
    pub(crate) fn render_ro_lock_button(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity();
        Button::new("editor-ro-lock")
            .ghost()
            .small()
            .icon(IconName::Lock)
            .toggled(self.user_ro_lock)
            .debug_selector(|| "editor-ro-lock".to_string())
            .tooltip(if self.user_ro_lock {
                "只读（点击切换到编辑）"
            } else {
                "编辑（点击切换到只读）"
            })
            .on_click(move |_, _window, app| {
                entity.update(app, |panel, cx| panel.toggle_user_ro_lock(cx));
            })
    }

    /// 【2d-笔记级】会话 ▾ 按钮：绑定连接的切换入口（笔记级事实）。
    ///
    /// 菜单 = 连接列表（当前会话绑定勾选）+「跟随当前活动连接」；空列表时保持
    /// 「跟随当前活动连接」可点（与 SQL 模式 B1 同口径，不假装有连接）。
    pub(crate) fn render_notebook_session_button(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity();
        let options = self.shared.connection_options();
        let current = self.notebook_state().session.default_connection.clone();
        // 会话绑定连接展示名（`None` = 跟随当前活动连接）
        let label = match &current {
            Some(id) => options
                .iter()
                .find(|c| &c.id == id)
                .map(|c| c.name.clone())
                .unwrap_or_else(|| format!("连接 {id}")),
            None => "跟随当前连接".to_string(),
        };
        Button::new("editor-notebook-session")
            .ghost()
            .small()
            .debug_selector(|| "editor-notebook-session".to_string())
            .icon(IconName::Globe)
            .tooltip("会话连接")
            .label(format!("{label} ▾"))
            .dropdown_menu(move |menu, _window, _cx| {
                let mut menu = menu;
                // 第一项：跟随当前活动连接（默认态）
                let follow = PopupMenuItem::new("跟随当前活动连接")
                    .checked(current.is_none())
                    .on_click({
                        let entity = entity.clone();
                        move |_, _window, app| {
                            entity.update(app, |panel, cx| {
                                panel.notebook_switch_connection(None, cx);
                            });
                        }
                    });
                menu = menu.item(follow);
                for conn in &options {
                    let selected = current.as_deref() == Some(conn.id.as_str());
                    let item = PopupMenuItem::new(conn.name.clone())
                        .checked(selected)
                        .on_click({
                            let entity = entity.clone();
                            let conn_id = conn.id.clone();
                            move |_, _window, app| {
                                entity.update(app, |panel, cx| {
                                    panel.notebook_switch_connection(Some(conn_id.clone()), cx);
                                });
                            }
                        });
                    menu = menu.item(item);
                }
                menu
            })
    }

    /// 【2d-笔记级】全部运行按钮：顺序跑所有可执行单元（SQL / 内核）。
    pub(crate) fn render_notebook_run_all_button(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity();
        Button::new("editor-notebook-run-all")
            .ghost()
            .small()
            .debug_selector(|| "editor-notebook-run-all".to_string())
            .icon(IconName::Play)
            .tooltip("全部运行")
            .on_click(move |_, _window, app| {
                entity.update(app, |panel, cx| panel.notebook_run_all(cx));
            })
    }

    /// 【2d-笔记级】重启按钮：关闭活跃内核（下次执行自动重拉）。
    pub(crate) fn render_notebook_restart_button(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity();
        Button::new("editor-notebook-restart")
            .ghost()
            .small()
            .debug_selector(|| "editor-notebook-restart".to_string())
            .icon(IconName::RefreshCw)
            .tooltip("重启内核")
            .on_click(move |_, _window, app| {
                entity.update(app, |panel, cx| panel.notebook_restart_kernel(cx));
            })
    }

    /// 当前文档是否为 Markdown（通用模式下的 `.md` 脚本类型）。决定「预览」切换是否出现。
    pub(crate) fn is_markdown_document(&self) -> bool {
        let Some(path) = self.document_path() else {
            return false;
        };
        crate::mode::script_file_type(&path) == Some(crate::mode::ScriptFileType::Markdown)
    }

    /// 【遗留】Markdown 文档「预览 / 源码」切换按钮（通用模式工具栏右侧）。
    /// 预览态 = 只读渲染视图（TextView::markdown），源码态 = 内核编辑器。
    pub(crate) fn render_markdown_preview_toggle(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity();
        let previewing = self.markdown_preview;
        Button::new("editor-markdown-preview-toggle")
            .ghost()
            .small()
            // 测试按选择器断言「预览态下渲染视图出现 / 源码态下编辑器出现」
            .debug_selector(|| "editor-markdown-preview-toggle".to_string())
            .label(if previewing { "源码" } else { "预览" })
            .toggled(previewing)
            .on_click(move |_, _window, app| {
                entity.update(app, |panel, cx| panel.toggle_markdown_preview(cx));
            })
    }

    /// 切换 Markdown 渲染态：预览 ↔ 源码编辑器（纯状态切换，渲染由 render 分支体现）。
    pub(crate) fn toggle_markdown_preview(&mut self, cx: &mut Context<Self>) {
        self.markdown_preview = !self.markdown_preview;
        cx.notify();
    }

    /// 【T2c-5】渲染一个组（组 = 分隔符界定的连续段，见 [`crate::toolbar`]）
    ///
    /// - 折叠（且组内可见成员 ≥ 2）：显示为「首组件 + ▾」——▾ 收纳组内其余**可见**成员；
    ///   这正是旧 `render_exec_group` 的形态，所以折叠 / 展开是同一组的两副面孔。
    /// - 展开：逐可见项渲染（执行 / 格式化 / 执行计划等独立按钮）。
    pub(crate) fn render_toolbar_group(
        &self,
        group: &crate::toolbar::ToolbarGroup,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let visible: Vec<crate::toolbar::ToolbarItemId> = group.visible_items(&self.toolbar);
        let collapsed = visible.len() >= 2 && self.toolbar.is_collapsed(group.head);
        if collapsed {
            self.render_collapsed_group(group.head, &visible, cx)
                .into_any_element()
        } else {
            let mut row = div().h_flex().items_center().gap_1();
            for id in &visible {
                row = row.child(self.render_toolbar_item(*id, cx));
            }
            row.into_any_element()
        }
    }

    /// 【T2c-5】渲染一个单项（目录 [`crate::toolbar::ToolbarItemId`]；动作统一走
    /// [`Self::run_toolbar_item_action`]——渲染只按 id 挂事件，动作语义集中在那一处）
    pub(crate) fn render_toolbar_item(
        &self,
        id: crate::toolbar::ToolbarItemId,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        match id {
            crate::toolbar::ToolbarItemId::ExecRun => self.render_exec_run_button(cx).into_any_element(),
            crate::toolbar::ToolbarItemId::ExecAll => {
                self.render_exec_all_button(cx).into_any_element()
            }
            crate::toolbar::ToolbarItemId::ExecCurrent => self
                .render_exec_kind_button(id, ExecMenuKind::CurrentStatement, cx)
                .into_any_element(),
            crate::toolbar::ToolbarItemId::ExecSelection => self
                .render_exec_kind_button(id, ExecMenuKind::Selection, cx)
                .into_any_element(),
            crate::toolbar::ToolbarItemId::ExecBatch => self
                .render_exec_kind_button(id, ExecMenuKind::Batch, cx)
                .into_any_element(),
            crate::toolbar::ToolbarItemId::ExecNewSet => self
                .render_exec_kind_button(id, ExecMenuKind::NewSet, cx)
                .into_any_element(),
            crate::toolbar::ToolbarItemId::Explain => {
                self.render_explain_button(cx).into_any_element()
            }
            crate::toolbar::ToolbarItemId::Translate => {
                self.render_translate_button(cx).into_any_element()
            }
            crate::toolbar::ToolbarItemId::Template => {
                self.render_template_button(cx).into_any_element()
            }
            crate::toolbar::ToolbarItemId::TxAutoCommit => {
                self.render_tx_auto_commit_button(cx).into_any_element()
            }
            crate::toolbar::ToolbarItemId::Format => {
                self.render_format_button(cx).into_any_element()
            }
            crate::toolbar::ToolbarItemId::History => {
                self.render_history_button(cx).into_any_element()
            }
        }
    }

    /// 【T2c-5】执行主按钮：与 `Ctrl+Enter` 走同一条路（选区优先 → 当前语句）
    pub(crate) fn render_exec_run_button(&self, cx: &mut Context<Self>) -> Button {
        let entity = cx.entity();
        Button::new("editor-exec-run")
            .primary()
            .small()
            .debug_selector(|| "editor-exec-run".to_string())
            .icon(crate::toolbar::ToolbarItemId::ExecRun.icon())
            .tooltip("执行（Ctrl+Enter）")
            .on_click(move |_, _window, app| {
                entity.update(app, |panel, cx| panel.execute_preferring_selection(cx));
            })
    }

    /// 【T2c-5】全部运行：与 `Ctrl+Shift+Enter` 同一条路（整篇脚本）
    pub(crate) fn render_exec_all_button(&self, cx: &mut Context<Self>) -> Button {
        let entity = cx.entity();
        Button::new("editor-exec-all")
            .ghost()
            .small()
            .debug_selector(|| "editor-exec-all".to_string())
            .icon(crate::toolbar::ToolbarItemId::ExecAll.icon())
            .tooltip("全部运行（Ctrl+Shift+Enter）")
            .on_click(move |_, _window, app| {
                entity.update(app, |panel, cx| panel.run_all(cx));
            })
    }

    /// 【T2c-5】执行族的显式目标按钮（当前语句 / 选区 / 批量）：`target_for_menu` 解析，
    /// 与旧下拉菜单的**显式目标**语义一致——“执行选区”没选区时置灰、“批量执行”语句不足
    /// 两条时置灰，比“点了没反应”明确。
    pub(crate) fn render_exec_kind_button(
        &self,
        id: crate::toolbar::ToolbarItemId,
        kind: ExecMenuKind,
        cx: &mut Context<Self>,
    ) -> Button {
        let entity = cx.entity();
        let (_, selection) = self.editor_snapshot(cx);
        let statements = self.statements;
        let available = kind.is_available(&selection, statements);
        let selector = match id {
            crate::toolbar::ToolbarItemId::ExecCurrent => "editor-exec-current",
            crate::toolbar::ToolbarItemId::ExecSelection => "editor-exec-selection",
            crate::toolbar::ToolbarItemId::ExecBatch => "editor-exec-batch",
            crate::toolbar::ToolbarItemId::ExecNewSet => "editor-exec-newset",
            _ => unreachable!("执行族显式目标按钮只有这四种"),
        };
        Button::new(selector)
            .ghost()
            .small()
            .debug_selector(move || selector.to_string())
            .icon(id.icon())
            .tooltip(id.label())
            .disabled(!available)
            .on_click(move |_, _window, app| {
                entity.update(app, |panel, cx| panel.execute_menu_kind(kind, cx));
            })
    }

    /// 【T2c-5】折叠组：首组件 + ▾，▾ 收纳组内其余可见成员
    ///
    /// ▾ 菜单只收执行族（`Exec*`）：格式化要 `&mut Window` 而菜单回调拿不到可靠的窗口，
    /// ⋯更多本身是嵌套菜单——两都放不进折叠菜单。执行族动作都不需要窗口，收进来没问题。
    ///
    /// 组头（`head`）必须是可 `Button` 化的项；`⋯ 更多` 由 [`Self::render_toolbar_group`]
    /// 提前退化为展开，到不了这里。
    pub(crate) fn render_collapsed_group(
        &self,
        head: crate::toolbar::ToolbarItemId,
        visible: &[crate::toolbar::ToolbarItemId],
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        use crate::toolbar::ToolbarItemId;
        let entity = cx.entity();
        let (_, selection) = self.editor_snapshot(cx);
        let statements = self.statements;
        // ElementId 用业务键（组头）不用下标：折叠多组时也互不冲突
        let key = format!("editor-toolbar-{:?}", head);
        let head_button: Button = match head {
            ToolbarItemId::ExecRun => self.render_exec_run_button(cx),
            ToolbarItemId::ExecAll => self.render_exec_all_button(cx),
            ToolbarItemId::ExecCurrent => {
                self.render_exec_kind_button(head, ExecMenuKind::CurrentStatement, cx)
            }
            ToolbarItemId::ExecSelection => {
                self.render_exec_kind_button(head, ExecMenuKind::Selection, cx)
            }
            ToolbarItemId::ExecBatch => self.render_exec_kind_button(head, ExecMenuKind::Batch, cx),
            ToolbarItemId::ExecNewSet => self.render_exec_kind_button(head, ExecMenuKind::NewSet, cx),
            ToolbarItemId::Explain => self.render_explain_button(cx),
            // 折叠组头只放纯按钮：下拉菜单由 DropdownButton 的 ▾ 半区提供，
            // 完整「方言转换 ▾ / 模板 ▾」本体见 render_translate_button / render_template_button
            ToolbarItemId::Translate => Button::new("editor-translate")
                .ghost()
                .small()
                .icon(ToolbarItemId::Translate.icon())
                .tooltip(ToolbarItemId::Translate.label()),
            ToolbarItemId::Template => Button::new("editor-template")
                .ghost()
                .small()
                .icon(ToolbarItemId::Template.icon())
                .tooltip(ToolbarItemId::Template.label())
                .disabled(self.shared.completion_templates().is_empty()),
            ToolbarItemId::TxAutoCommit => self.render_tx_auto_commit_button(cx),
            ToolbarItemId::Format => self.render_format_button(cx),
            ToolbarItemId::History => self.render_history_button(cx),
        };
        let menu_items: Vec<ToolbarItemId> = visible
            .iter()
            .copied()
            .filter(|id| {
                *id != head
                    && matches!(
                        *id,
                        ToolbarItemId::ExecRun
                            | ToolbarItemId::ExecAll
                            | ToolbarItemId::ExecCurrent
                            | ToolbarItemId::ExecSelection
                            | ToolbarItemId::ExecBatch
                    )
            })
            .collect();
        DropdownButton::new(key)
            .small()
            .button(head_button)
            .dropdown_menu(move |menu, _window, _cx| {
                let mut menu = menu;
                for id in menu_items.iter() {
                    let entity = entity.clone();
                    let kind = match id {
                        ToolbarItemId::ExecRun => ExecMenuKind::All,
                        ToolbarItemId::ExecAll => ExecMenuKind::All,
                        ToolbarItemId::ExecCurrent => ExecMenuKind::CurrentStatement,
                        ToolbarItemId::ExecSelection => ExecMenuKind::Selection,
                        ToolbarItemId::ExecBatch => ExecMenuKind::Batch,
                        _ => unreachable!("折叠菜单只收执行族"),
                    };
                    let available = kind.is_available(&selection, statements);
                    let id = *id;
                    menu = menu.item(
                        PopupMenuItem::new(id.label())
                            .disabled(!available)
                            .on_click(move |_, window, app| {
                                entity.update(app, |panel, cx| {
                                    panel.run_toolbar_item_action(id, window, cx);
                                });
                            }),
                    );
                }
                menu
            })
    }

}

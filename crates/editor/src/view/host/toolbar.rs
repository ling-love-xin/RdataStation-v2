//! 编辑器宿主面板 —— 工具栏渲染与动作（render_toolbar/render_toolbar_group/工具栏按钮/sync_mode/on_save/工具栏自定义）
//!
//! 自 host.rs 拆分（2026-10-29）：按能力域切分 `impl EditorHostPanel` 方法块。
//! 跨域互调经 `pub(crate)` 方法；字段在 mod.rs 统一 `pub(crate)`。
//!
//! 拆分纪律：只移动方法体与注释，不改动任何行为；本文件不得出现新的逻辑。


use gpui_kit::assets::IconName;
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::popover::Popover;
use gpui_kit::component::separator::Separator;
// #48 编辑器常规设置（行号开关即时 / 新文档默认缩进·换行·编码）：settings 不依赖 editor，方向合法

use gpui_kit::component::switch::Switch;
use gpui_kit::component::toolbar::Toolbar;

use crate::channel::ExecChannel;
use crate::commands::{
    FormatDocument, SaveDocument, ToggleComment,
};
use crate::edit;
use crate::execution::{self, ExecMenuKind};
use crate::model::EditorMode;
// 【下移】结果放置 / 下发原料定义已随结果存储搬到 rds-result-panel（经 crate::store 重导出）
use crate::translate;
use crate::ui;
use crate::view::completion;
use crate::view::highlight;


use super::*;
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
    fn render_ro_lock_button(&self, cx: &mut Context<Self>) -> impl IntoElement {
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
    fn render_notebook_session_button(&self, cx: &mut Context<Self>) -> impl IntoElement {
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
    fn render_notebook_run_all_button(&self, cx: &mut Context<Self>) -> impl IntoElement {
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
    fn render_notebook_restart_button(&self, cx: &mut Context<Self>) -> impl IntoElement {
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
    fn render_markdown_preview_toggle(&self, cx: &mut Context<Self>) -> impl IntoElement {
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
    fn render_toolbar_group(
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
    fn render_toolbar_item(
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
    fn render_exec_run_button(&self, cx: &mut Context<Self>) -> Button {
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
    fn render_exec_all_button(&self, cx: &mut Context<Self>) -> Button {
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
    fn render_exec_kind_button(
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
    fn render_collapsed_group(
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

    /// 【B10】文档级：格式化按钮（原型 §2.2 的「⇤ 格式化」，紧随执行级）
    fn render_format_button(&self, cx: &mut Context<Self>) -> Button {
        let entity = cx.entity();
        Button::new("editor-format")
            .ghost()
            .small()
            .debug_selector(|| "editor-format".to_string())
            .icon(crate::toolbar::ToolbarItemId::Format.icon())
            .tooltip("格式化")
            .on_click(move |_, window, app| {
                entity.update(app, |panel, cx| panel.format_document(window, cx));
            })
    }

    /// 【B8 余项】文档级「⟲ 历史」：打开右 Dock 历史面板（端口注入——编辑器不依赖工作台
    /// 的 Dock）。未接端口时按钮置灰（`.disabled`），点了也会在状态栏给可读原因（不静默）。
    fn render_history_button(&self, cx: &mut Context<Self>) -> Button {
        let entity = cx.entity();
        let available = self.shared.open_history_port().is_some();
        Button::new("editor-history")
            .ghost()
            .small()
            .disabled(!available)
            .debug_selector(|| "editor-history".to_string())
            .icon(crate::toolbar::ToolbarItemId::History.icon())
            .tooltip("历史")
            .on_click(move |_, _window, app| {
                if entity.read(app).shared.open_history_port().is_none() {
                    // 未接端口：状态栏给原因，而不是“点了没反应”
                    entity.update(app, |panel, cx| {
                        panel.set_message(Some("未接入历史面板".to_string()), cx);
                    });
                    return;
                }
                let port = entity.read(app).shared.open_history_port().unwrap();
                port(app);
            })
    }

    /// 【原型 §2.2 分析组】执行计划（EXPLAIN）：常驻按钮（2026-10-11 提升常驻，不再进"更多"）
    fn render_explain_button(&self, cx: &mut Context<Self>) -> Button {
        let entity = cx.entity();
        Button::new("editor-explain")
            .ghost()
            .small()
            .debug_selector(|| "editor-explain".to_string())
            .icon(crate::toolbar::ToolbarItemId::Explain.icon())
            .tooltip("执行计划")
            .on_click(move |_, _window, app| {
                entity.update(app, |panel, cx| panel.explain_current(cx));
            })
    }

    /// 【原型 §2.2 分析组】方言转换：常驻按钮（目标方言列表）
    ///
    /// 目标清单在**渲染时**定好（源方言已知就排除它自己）；菜单回调是 `Fn`，不能在里头读面板。
    /// 源方言未知（未绑定连接）时仍把目标列出来——点下去会收到一句可读的拒绝，
    /// 比“菜单里什么都没有”更容易看懂。
    fn render_translate_button(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity();
        let targets: Vec<translate::Target> = match self.source_dialect() {
            Ok(source) => translate::targets_for(source),
            Err(_) => translate::TARGETS.to_vec(),
        };
        Button::new("editor-translate")
            .ghost()
            .small()
            .debug_selector(|| "editor-translate".to_string())
            .icon(crate::toolbar::ToolbarItemId::Translate.icon())
            .tooltip("方言转换")
            .dropdown_menu(move |menu, window, cx| {
                menu.submenu("转译为", window, cx, {
                    let entity = entity.clone();
                    let targets = targets.clone();
                    move |sub, _window, _cx| {
                        let mut sub = sub;
                        for target in targets.iter() {
                            let entity = entity.clone();
                            let dialect = target.dialect;
                            sub = sub.item(PopupMenuItem::new(target.label).on_click(
                                move |_, window, app| {
                                    entity.update(app, |panel, cx| {
                                        panel.transpile_document(dialect, window, cx);
                                    });
                                },
                            ));
                        }
                        sub
                    }
                })
            })
    }

    /// 【原型 §2.2 事务/模板】模板 / 代码片段：常驻按钮（无模板时置灰，菜单不摆空项）
    fn render_template_button(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity();
        // 【B9 切片二】模板片段（内存读：渲染时读一次；没模板 / 没接端口就置灰）
        let templates = self.shared.completion_templates();
        let has_templates = !templates.is_empty();
        Button::new("editor-template")
            .ghost()
            .small()
            .debug_selector(|| "editor-template".to_string())
            .disabled(!has_templates)
            .icon(crate::toolbar::ToolbarItemId::Template.icon())
            .tooltip("模板")
            .dropdown_menu(move |menu, _window, _cx| {
                let mut menu = menu;
                for (index, template) in templates.iter().enumerate() {
                    let entity = entity.clone();
                    menu = menu.item(PopupMenuItem::new(template.name.clone()).on_click(
                        move |_, window, app| {
                            entity.update(app, |panel, cx| {
                                panel.insert_template(index, window, cx);
                            });
                        },
                    ));
                }
                menu
            })
    }

    /// 【原型 §2.2 事务/模板】事务提交方式（自动 ⇄ 手动）：常驻开关（模式要一直看得见）
    ///
    /// 从状态栏 TX 区移到工具栏（原型 §2.2 的“事务开关常显”）；状态栏 TX 区只留
    /// 状态文本 + 事务开启时的提交 / 回滚。
    fn render_tx_auto_commit_button(&self, cx: &mut Context<Self>) -> Button {
        let entity = cx.entity();
        Button::new("editor-autocommit")
            .ghost()
            .small()
            .debug_selector(|| "editor-autocommit".to_string())
            .toggled(self.autocommit)
            .icon(crate::toolbar::ToolbarItemId::TxAutoCommit.icon())
            .tooltip("自动提交")
            .on_click(move |_, _window, app| {
                entity.update(app, |panel, cx| panel.toggle_autocommit(cx));
            })
    }

    /// 【T2c-5】`···` 定制按钮 + 三栏设置面板（可见性 / 折叠 / 移动 / 竖线分隔符 / 重置）
    ///
    /// 为什么用 `Popover` 而不是 `DropdownMenu`：菜单实体是**缓存**的，切换可见性后内容会
    /// 陈旧；`Popover::content` 的闭包**每帧重建**，能实时反映面板状态。面板不是菜单——
    /// 它是「图标功能｜可见｜移动」三栏的设置视图，用 `Checkbox` / `Switch` / `Button` /
    /// `Separator` 现成组件拼装（组件禁手搓）。
    ///
    /// 面板分四段：标题 → 组头行（组名 + 折叠开关 + 组移动↑↓）→ 成员行（图标+标签｜
    /// 可见 ▢｜单项移动↑↓）→ 段尾分隔符行（┃ 竖线分隔符 + ✕）→ 底部（＋添加竖线分隔符｜
    /// ↺ 重置默认）。所有动作走面板方法（事件路径），render 只按 id 挂回调。
    fn render_toolbar_customize(&self, cx: &mut Context<Self>) -> impl IntoElement {
        use crate::toolbar::MoveDir;
        let entity = cx.entity();
        let panel_width = rems(ui::TOOLBAR_CUSTOMIZE_WIDTH);
        Popover::new("editor-toolbar-customize-popover")
            // 最右的按钮：面板向右下展开，避免被窗口右缘截断
            .anchor(Anchor::BottomRight)
            .offset(ui::TOOLBAR_CUSTOMIZE_OFFSET)
            .trigger(
                Button::new("editor-toolbar-customize")
                    .ghost()
                    .small()
                    .debug_selector(|| "editor-toolbar-customize".to_string())
                    .icon(IconName::Ellipsis)
                    .tooltip("自定义工具栏"),
            )
            .content(move |_state, _window, cx| {
                let muted = cx.theme().colors.muted_foreground;
                let panel = entity.read(cx);
                let layout = &panel.toolbar;
                let groups = layout.groups();

                let mut body = div()
                    .v_flex()
                    .gap_1()
                    .p_2()
                    .w(panel_width)
                    .debug_selector(|| "editor-toolbar-customize-panel".to_string());
                body = body.child(
                    div()
                        .px_1()
                        .text_xs()
                        .text_color(muted)
                        .child("自定义工具栏"),
                );
                body = body.child(Separator::horizontal());
                // 竖线分隔符位置快照：分离出来，避免 `'static` 闭包借用临时值
                let separator_positions = layout.separator_positions();

                for group in &groups {
                    // 组头行：组名 + 折叠开关 + 组移动↑↓
                    let head = group.head;
                    let collapsed = layout.is_collapsed(head);
                    let can_move_left = head != group_of_first(&groups);
                    let can_move_right = head != group_of_last(&groups);
                    body = body.child(
                        div()
                            .h_flex()
                            .items_center()
                            .gap_1()
                            .debug_selector(move || {
                                format!("editor-toolbar-customize-group-{:?}", head)
                            })
                            .child(panel.item_icon_row(head, cx))
                            .child(
                                div()
                                    .flex_1()
                                    .text_xs()
                                    .text_color(muted)
                                    .child(head.label()),
                            )
                            .child(Switch::new(format!("toolbar-fold-{:?}", head))
                                .checked(collapsed)
                                .label("折叠")
                                .on_click({
                                    let entity = entity.clone();
                                    move |_, _window, app| {
                                        entity.update(app, |panel, cx| {
                                            panel.toggle_toolbar_collapsed(head, cx);
                                        });
                                    }
                                }))
                            .child(panel.small_icon_button(
                                &entity,
                                format!("toolbar-group-up-{:?}", head),
                                IconName::ChevronUp,
                                can_move_left,
                                move |panel, cx| panel.move_toolbar_group(head, MoveDir::Left, cx),
                            ))
                            .child(panel.small_icon_button(
                                &entity,
                                format!("toolbar-group-down-{:?}", head),
                                IconName::ChevronDown,
                                can_move_right,
                                move |panel, cx| panel.move_toolbar_group(head, MoveDir::Right, cx),
                            )),
                    );
                    // 成员行：图标+标签｜可见 ▢｜单项移动↑↓
                    for id in &group.items {
                        let id = *id;
                        let visible = layout.is_visible(id);
                        let fixed = id.fixed_visible();
                        body = body.child(
                            div()
                                .h_flex()
                                .items_center()
                                .gap_1()
                                .pl_3()
                                .debug_selector(move || {
                                    format!("editor-toolbar-customize-item-{:?}", id)
                                })
                                .child(panel.item_icon_row(id, cx))
                                .child(div().flex_1().text_xs().child(id.label()))
                                .child(
                                    Checkbox::new(format!("toolbar-vis-{:?}", id))
                                        .checked(visible)
                                        .disabled(fixed)
                                        .on_click({
                                            let entity = entity.clone();
                                            move |&on, _window, app| {
                                                entity.update(app, |panel, cx| {
                                                    panel.set_toolbar_visible(id, on, cx);
                                                });
                                            }
                                        }),
                                )
                                .child(panel.small_icon_button(
                                    &entity,
                                    format!("toolbar-item-up-{:?}", id),
                                    IconName::ChevronUp,
                                    true,
                                    move |panel, cx| panel.move_toolbar_item(id, MoveDir::Left, cx),
                                ))
                                .child(panel.small_icon_button(
                                    &entity,
                                    format!("toolbar-item-down-{:?}", id),
                                    IconName::ChevronDown,
                                    true,
                                    move |panel, cx| {
                                        panel.move_toolbar_item(id, MoveDir::Right, cx)
                                    },
                                )),
                        );
                    }
                }

                // 段尾分隔符行：每条竖线分隔符一个「┃ 竖线分隔符 + ✕」
                for (index, pos) in separator_positions.iter().enumerate() {
                    let key = format!("toolbar-sep-{index}");
                    let pos = *pos;
                    body = body.child(
                        div()
                            .h_flex()
                            .items_center()
                            .gap_1()
                            .pl_3()
                            .debug_selector(move || key.clone())
                            .child(Separator::vertical().h(rems(0.75)))
                            .child(div().flex_1().text_xs().text_color(muted).child("竖线分隔符"))
                            .child(panel.small_icon_button(
                                &entity,
                                format!("toolbar-sep-up-{index}"),
                                IconName::ChevronUp,
                                true,
                                move |panel, cx| {
                                    panel.move_toolbar_separator(pos, MoveDir::Left, cx)
                                },
                            ))
                            .child(panel.small_icon_button(
                                &entity,
                                format!("toolbar-sep-down-{index}"),
                                IconName::ChevronDown,
                                true,
                                move |panel, cx| {
                                    panel.move_toolbar_separator(pos, MoveDir::Right, cx)
                                },
                            ))
                            .child(panel.small_icon_button(
                                &entity,
                                format!("toolbar-sep-remove-{index}"),
                                IconName::X,
                                true,
                                move |panel, cx| panel.remove_toolbar_separator(pos, cx),
                            )),
                    );
                }

                body = body.child(Separator::horizontal());
                // 底部：＋添加竖线分隔符｜↺ 重置默认
                body = body.child(
                    div()
                        .h_flex()
                        .items_center()
                        .gap_1()
                        .child(
                            Button::new("editor-toolbar-customize-add-sep")
                                .ghost()
                                .small()
                                .icon(IconName::Plus)
                                .label("添加竖线分隔符")
                                .on_click({
                                    let entity = entity.clone();
                                    move |_, _window, app| {
                                        entity.update(app, |panel, cx| {
                                            panel.insert_toolbar_separator(cx);
                                        });
                                    }
                                }),
                        )
                        .child(
                            Button::new("editor-toolbar-customize-reset")
                                .ghost()
                                .small()
                                .icon(IconName::RotateCcw)
                                .label("重置默认")
                                .on_click({
                                    let entity = entity.clone();
                                    move |_, _window, app| {
                                        entity.update(app, |panel, cx| panel.reset_toolbar(cx));
                                    }
                                }),
                        ),
                );
                body
            })
    }

    /// 【T2c-5】面板成员行的最左列：功能图标（挂 `editor-toolbar-customize-icon-*` 选择器）
    ///
    /// 从 Popover content 闭包里调：那儿的 `cx` 是 `Context<PopoverState>`，所以只收 `&App`
    /// 取主题色，不做任何面板状态的读写（状态由 content 闭包在外层 `entity.read` 统一读）。
    fn item_icon_row(
        &self,
        id: crate::toolbar::ToolbarItemId,
        cx: &gpui_kit::App,
    ) -> impl IntoElement {
        let muted = cx.theme().colors.muted_foreground;
        let key = format!("editor-toolbar-customize-icon-{:?}", id);
        div()
            .w_4()
            .h_flex()
            .items_center()
            .justify_center()
            .debug_selector(move || key.clone())
            .child(Icon::new(id.icon()).text_color(muted))
    }

    /// 【T2c-5】面板里的小图标按钮（↑↓ / ✕ 用；统一 ghost + small + 图标）
    ///
    /// 为什么显式传 `entity` 而不是 `cx.entity()`：Popover content 闭包里的 `cx` 是
    /// `Context<PopoverState>`，拿不到面板实体——由调用方把捕获的 `entity` 传进来，
    /// 动作统一 `entity.update` 转交面板方法。
    fn small_icon_button(
        &self,
        entity: &Entity<Self>,
        id: impl Into<gpui_kit::ElementId>,
        icon: IconName,
        enabled: bool,
        action: impl Fn(&mut Self, &mut Context<Self>) + 'static,
    ) -> Button {
        let entity = entity.clone();
        Button::new(id)
            .ghost()
            .small()
            .icon(icon)
            .disabled(!enabled)
            .on_click(move |_, _window, app| {
                entity.update(app, |panel, cx| action(panel, cx));
            })
    }

    pub(crate) fn editor_read_only(&self) -> bool {
        // 用户只读开关（roLock）与文档只读合并：任一为只读 → 编辑器只读
        self.user_ro_lock
            || self
                .with_document(|doc| !doc.read_only().can_edit())
                .unwrap_or(true)
    }

    /// 模式切换后同步视图侧状态（高亮是否启用、只读）
    ///
    /// 模式是**文档属性**：判定与确认在 `mode::plan_switch` 与对话框层，这里只负责
    /// “按当前模式刷新视图”，由切换流程在确认之后调用（**视图不自己改模式**）。
    pub fn sync_mode(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(mode) = self.with_document(|doc| doc.mode()) else {
            return;
        };
        let read_only = self.editor_read_only();
        // 语义着色类型：模式切换后按新模式与文档扩展名重算（脚本分派 / CSV 不上色）
        let highlight_kind =
            self.with_document(|doc| highlight::kind_for_document(doc.path(), doc.mode()))
                .unwrap_or(highlight::HighlightKind::None);

        // 【LSP】模式切换时重新向宿主要服务（插件加载 / 宿主判定变了能接上；没变 = 无操作）
        self.sync_lsp_session(cx);

        self.editor.update(cx, |state, cx| {
            state.set_readonly(read_only, cx);
            if highlight::is_enabled(mode) {
                highlight::install(state, cx, highlight_kind);
            } else {
                // 未启用的模式（当前只剩分析）：摘掉着色器，不再取新 token。内核的
                // token 缓存没有公开的清入口，摘除后旧 token 要等下一次文本变更才失效。
                state.lsp_mut().semantic_tokens_provider = None;
            }
            // B9：补全按能力表装 / 摘（文本模式不接，分析模式留 1c 逐单元）
            self.sync_completion_provider(state);
            // B17：折叠按能力表与档位装 / 关（分析模式能力位为 false → 真关）
            sync_fold(&self.shared, &self.document, state, window, cx);
        });
        cx.notify();
        // 模式变了 = 语言 / 环境 / 能力徽标全变：通知宿主刷新全局段
        cx.emit(EditorHostEvent::StatusChanged);
    }

    /// `Ctrl+S`：保存；未命名 / 写盘失败 → 状态栏给出原因
    pub(crate) fn on_save(
        &mut self,
        _: &SaveDocument,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        tracing::info!(doc = %self.document, "【探针】on_save：保存文档（Ctrl+S）");
        // 【格式化增强】保存时自动格式化（设置开）：先格式化再落盘——内核 Change 事件
        // 会把新文本写回服务层，`save` 拿到的就是格式化后的内容。
        // 格式化内部的提示消息会被下面的成功 / 失败分支覆盖，不打扰保存反馈。
        if self.format_on_save_enabled(cx) {
            self.format_document(window, cx);
        }
        match self.save(cx) {
            Ok(_) => {
                // A12：保存时顺便把会话（光标/选区/模式）落库——用户按 Ctrl+S 是最自然的时机
                self.save_session_now(cx);
                self.set_message(None, cx);
            }
            Err(error) => self.set_message(Some(error.to_string()), cx),
        }
    }

    /// `Ctrl+Shift+F`：格式化（整篇 / 选区）
    ///
    /// 动作本身极薄：格式化是**文档级**动作，真伪都由 [`Self::format_document`] 判定
    /// （只读拒绝 / 文本模式拒绝 / 解析不了的语句原样保留）。
    pub(crate) fn on_format(
        &mut self,
        _: &FormatDocument,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        tracing::info!(doc = %self.document, "【探针】on_format：格式化文档（Ctrl+Shift+F）");
        self.format_document(window, cx);
    }

    /// `Ctrl+Space`：手动请一次补全（B9 切片二）
    ///
    /// 打字触发那条路由内核管（`is_completion_trigger`）；这条是**用户明确要候选**——
    /// 所以候选与打字那条**同一份**（`view/completion.rs::items_at`），只是替换范围改由
    /// 我们算（光标前的标识符）。拒绝的理由要说出来（文本模式 / 只读 / 没候选）。
    pub(crate) fn on_trigger_completion(
        &mut self,
        _: &crate::commands::TriggerCompletion,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        tracing::info!(doc = %self.document, "【探针】on_trigger_completion：手动请求补全（Ctrl+Space）");
        self.trigger_completion(cx);
    }

    /// 手动补全的落地（动作与测试都走它）
    pub(crate) fn trigger_completion(&mut self, cx: &mut Context<Self>) {
        if !self.shared.completion_enabled(&self.document) {
            // 三档拒绝理由分开说：文本模式 / 只读 / 大文件档位——“按了没反应”是不允许的
            let reason = self
                .with_document(|doc| {
                    if doc.mode() != EditorMode::Sql {
                        Some("文本模式不解析 SQL（补全是 SQL 模式的能力）".to_string())
                    } else if !doc.read_only().can_edit() {
                        Some("文档只读，无法补全".to_string())
                    } else if doc.tier().disables_completion() {
                        Some("文件太大（超大文件档位关掉了补全）".to_string())
                    } else {
                        None
                    }
                })
                .flatten()
                .unwrap_or_else(|| "当前不可补全".to_string());
            self.set_message(Some(reason), cx);
            return;
        }
        let (text, offset) = {
            let state = self.editor.read(cx);
            (state.value().to_string(), state.cursor())
        };
        let (start, query, items) =
            completion::items_at(&self.shared, &self.document, &text, offset);
        if items.is_empty() {
            self.set_message(
                Some("没有可补的候选（元数据可能还没载好，或光标处认不出上下文）".to_string()),
                cx,
            );
            return;
        }
        self.editor.update(cx, |state, cx| {
            state.present_completion_items(start, query, items, cx);
        });
        self.set_message(None, cx);
    }

    /// 【B9 切片二】插入一条模板片段（「⋯ 更多 ▾ ▸ 插入模板」调用）
    ///
    /// 插入后**选中 `{table}` 占位符**：用户接着敲表名就把它覆写了。
    /// 只读与文本模式在这里就拒（与其它写文本的动作同口径）。
    pub(crate) fn insert_template(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.editor_read_only() {
            self.set_message(Some("文档只读，无法插入模板".to_string()), cx);
            return;
        }
        let templates = self.shared.completion_templates();
        let Some(template) = templates.get(index) else {
            return;
        };
        let content = template.content.clone();
        let name = template.name.clone();
        let start = {
            let state = self.editor.read(cx);
            state.cursor()
        };
        self.editor
            .update(cx, |state, cx| state.insert(content.clone(), window, cx));
        if let Some((from, to)) = crate::completion::placeholder_range(&content) {
            self.editor.update(cx, |state, cx| {
                state.set_selected_range(start + from..start + to, cx)
            });
        }
        self.set_message(Some(format!("已插入模板「{name}」")), cx);
    }

    /// 【T2c-5】工具栏单项动作的统一分发（渲染只按 id 挂事件，语义集中在这里）
    ///
    /// 折叠 ▾ 菜单点击项也走这里（执行族）。`More` 是嵌套菜单本身、不参与折叠菜单，
    /// 这里兜底空转（不会真被调到——`render_collapsed_group` 已把它过滤掉）。
    /// 格式化的 `&mut Window` 从事件回调（按钮 / 菜单项都带窗口）透传进来。
    pub(crate) fn run_toolbar_item_action(
        &mut self,
        id: crate::toolbar::ToolbarItemId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use crate::toolbar::ToolbarItemId;
        match id {
            ToolbarItemId::ExecRun => self.execute_preferring_selection(cx),
            ToolbarItemId::ExecAll => self.run_all(cx),
            ToolbarItemId::ExecCurrent => self.execute_menu_kind(ExecMenuKind::CurrentStatement, cx),
            ToolbarItemId::ExecSelection => self.execute_menu_kind(ExecMenuKind::Selection, cx),
            ToolbarItemId::ExecBatch => self.execute_menu_kind(ExecMenuKind::Batch, cx),
            ToolbarItemId::ExecNewSet => self.execute_menu_kind(ExecMenuKind::NewSet, cx),
            ToolbarItemId::Format => self.format_document(window, cx),
            // Explain / Translate / Template / TxAutoCommit / History 是带下拉的文档级
            // 按钮（`dropdown_menu` 回调内直接调方法，不走这条兜底）；折叠菜单只收执行族
            // （见 `render_collapsed_group`），这里兜底空转（不会真被调到）
            ToolbarItemId::Explain => {}
            ToolbarItemId::Translate => {}
            ToolbarItemId::Template => {}
            ToolbarItemId::TxAutoCommit => {}
            ToolbarItemId::History => {}
        }
    }

    /// 【T2c-5】按显式菜单目标执行（`target_for_menu`；工具栏显式目标按钮与折叠 ▾ 菜单共用）
    pub(crate) fn execute_menu_kind(&mut self, kind: ExecMenuKind, cx: &mut Context<Self>) {
        let (text, selection) = self.editor_snapshot(cx);
        let target = execution::target_for_menu(kind, &text, selection);
        self.execute(target, kind.placement(), cx);
    }

    /// 【T2c-5】面板：切换某项可见性（`fixed_visible` 项不可隐藏，面板上 Checkbox 置灰）
    ///
    /// 返回是否真的变了（测试断言「不可隐藏」用）。
    pub(crate) fn set_toolbar_visible(
        &mut self,
        id: crate::toolbar::ToolbarItemId,
        on: bool,
        cx: &mut Context<Self>,
    ) -> bool {
        let changed = self.toolbar.set_visible(id, on);
        cx.notify();
        changed
    }

    /// 【T2c-5】面板：单项移动（可跨组；跨过分隔符自然形成新组）
    pub(crate) fn move_toolbar_item(
        &mut self,
        id: crate::toolbar::ToolbarItemId,
        dir: crate::toolbar::MoveDir,
        cx: &mut Context<Self>,
    ) {
        self.toolbar.move_item(id, dir);
        cx.notify();
    }

    /// 【T2c-5】面板：整组移动（含段尾分隔符；`false` = 已在边缘）
    pub(crate) fn move_toolbar_group(
        &mut self,
        head: crate::toolbar::ToolbarItemId,
        dir: crate::toolbar::MoveDir,
        cx: &mut Context<Self>,
    ) {
        self.toolbar.move_group(head, dir);
        cx.notify();
    }

    /// 【T2c-5】面板：切换某组折叠（折叠 = 只显示首组件 + ▾）
    pub(crate) fn toggle_toolbar_collapsed(
        &mut self,
        head: crate::toolbar::ToolbarItemId,
        cx: &mut Context<Self>,
    ) {
        self.toolbar.toggle_collapsed(head);
        cx.notify();
    }

    /// 【T2c-5】面板：在左区末尾加一条竖线分隔符
    pub(crate) fn insert_toolbar_separator(&mut self, cx: &mut Context<Self>) {
        self.toolbar.insert_separator();
        cx.notify();
    }

    /// 【T2c-5】面板：移除某条竖线分隔符
    pub(crate) fn remove_toolbar_separator(&mut self, index: usize, cx: &mut Context<Self>) {
        self.toolbar.remove_separator(index);
        cx.notify();
    }

    /// 【T2c-5】面板：移动某条竖线分隔符（跨过一项即改变组边界）
    pub(crate) fn move_toolbar_separator(
        &mut self,
        index: usize,
        dir: crate::toolbar::MoveDir,
        cx: &mut Context<Self>,
    ) {
        self.toolbar.move_separator(index, dir);
        cx.notify();
    }

    /// 【T2c-5】面板：重置默认（清折叠 / 可见性回默认 / 删新增分隔符）
    pub(crate) fn reset_toolbar(&mut self, cx: &mut Context<Self>) {
        self.toolbar.reset();
        cx.notify();
    }

    /// 【T2c-5】测试访问器：当前工具栏布局快照（断言折叠 / 可见性 / 顺序）
    pub fn toolbar_layout_for_test(&self) -> crate::toolbar::ToolbarLayout {
        self.toolbar.clone()
    }

    /// `Ctrl+/`：行注释开关
    ///
    /// 写回内核走「选中块 → `replace` → 重新选中」：`replace` 会进撤销栈且保留历史，
    /// 而 `set_value` 会清掉撤销栈（注释必须可撤销，见 `edit` 模块说明）。
    pub(crate) fn on_toggle_comment(
        &mut self,
        _: &ToggleComment,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.editor_read_only() {
            self.set_message(Some("文档只读，无法注释".to_string()), cx);
            return;
        }
        let Some(mode) = self.with_document(|doc| doc.mode()) else {
            return;
        };
        let prefix = edit::comment_prefix(mode);
        let (text, selection) = {
            let state = self.editor.read(cx);
            (state.value().to_string(), state.selected_range())
        };

        let merged = edit::toggle_line_comment(&text, selection, prefix);
        if merged.text == text {
            // 全是空行：没有可注释的内容（不报错，也不留提示）
            return;
        }

        let new_text = merged.text.clone();
        self.editor.update(cx, |state, cx| {
            state.set_selected_range(merged.replaced.clone(), cx);
            state.replace(merged.block.clone(), window, cx);
            state.set_selected_range(merged.selection.clone(), cx);
        });
        // 程序化写入不保证发 `Change` 事件，这里主动同步服务层（同一值重复写入是幂等的）
        self.statements = count_statements(&new_text);
        self.shared
            .update(|service| service.set_content(&self.document, new_text));
        self.set_message(None, cx);
    }

    /// 把某个文件的内容插到**光标处**（拖放落点；原型 §4.5）。
    ///
    /// 只读文档直接拒绝；读盘失败 / 空文件都给一条消息，不静默。
    pub(crate) fn insert_file_contents(
        &mut self,
        payload: &::shared::InsertFileDrag,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.editor_read_only() {
            self.set_message(Some("文档只读，无法插入内容".to_string()), cx);
            return;
        }
        let text = match crate::persist::load(&payload.path) {
            Ok(text) => text,
            Err(error) => {
                self.set_message(Some(format!("插入失败：{error}")), cx);
                return;
            }
        };
        if text.is_empty() {
            self.set_message(
                Some(format!("{} 是空文件，没有可插入的内容", payload.label)),
                cx,
            );
            return;
        }
        // 在光标处插入：`replace` 替换当前选区，并把光标留在插入内容之后。
        self.editor
            .update(cx, |state, cx| state.replace(text, window, cx));
        // 程序化写入不保证发 `Change` 事件 → 主动同步服务层（与行注释同一口径，幂等）。
        let new_text = self.editor.read(cx).value().to_string();
        self.statements = count_statements(&new_text);
        self.shared
            .update(|service| service.set_content(&self.document, new_text));
        self.set_message(Some(format!("已插入 {}", payload.label)), cx);
    }

    // ===== 函数手册（#54-C5）=====
    //
    // Quick Open 式中文语义搜索（输入「截取」→ SUBSTR），确认后把函数名插到光标处。
    // 数据与排序全在 engine（157 条 / 9 类静态手册），editor 只做弹层与插入。

    /// `Ctrl+Shift+Space`：签名帮助（识别光标前函数调用 → 内置签名目录 → Dialog；
    /// 识别不出 / 查不到 = 静默，不打扰）
    pub(crate) fn on_signature_help(
        &mut self,
        _: &crate::commands::SignatureHelp,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        tracing::info!(doc = %self.document, "【探针】on_signature_help：签名帮助（Ctrl+Shift+Space）");
        let text = self.editor.read(cx).value().to_string();
        // 活动光标在前（多光标取第一个即可——签名帮助是"看当前调用"的辅助）
        let cursor = self.editor.read(cx).cursor_offsets().first().copied().unwrap_or(0);
        if let Some(view) = crate::view::signature_help::current_call(&text, cursor) {
            crate::view::signature_help::open_signature_help(window, cx, view);
        }
    }
    pub(crate) fn on_function_manual(
        &mut self,
        _: &crate::commands::FunctionManual,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        tracing::info!(doc = %self.document, "【探针】on_function_manual：打开函数手册对话框");
        _ = self.open_function_manual(window, cx);
    }

    /// 打开函数手册对话框（返回 ListState 实体，测试可直接驱动搜索/确认）
    ///
    /// 确认回调捕获宿主实体，把函数名交给 [`Self::insert_manual_function`]。
    pub(crate) fn open_function_manual(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<gpui_kit::component::list::ListState<
        crate::view::function_manual::ManualDelegate,
    >> {
        let entity = cx.entity();
        crate::view::function_manual::open_function_manual(
            window,
            cx,
            move |name, window, cx| {
                entity.update(cx, |panel, cx| {
                    panel.insert_manual_function(name, window, cx);
                });
            },
        )
    }

    /// 手册确认 → 光标处插入函数名 + 同步服务层（同「插入片段」口径，幂等）
    fn insert_manual_function(
        &mut self,
        name: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.editor
            .update(cx, |state, cx| state.insert(name.clone(), window, cx));
        let new_text = self.editor.read(cx).value().to_string();
        self.statements = count_statements(&new_text);
        self.shared
            .update(|service| service.set_content(&self.document, new_text));
        self.set_message(Some(format!("已插入函数 {}", name)), cx);
    }

    // ===== 模式切换（A5 的矩阵 + A9 的确认对话框）=====
    //
    // 入口是工具栏最左的**模式指示器**（原型 §2.2）：点击选目标模式，代价由 `mode::plan_switch`
    // 判定，需确认的先弹对话框；确认后本面板落地（写内容 + 改模式 + 刷新着色/只读）。
    // **禁止静默切换**：任何需要付出代价的切换都不过状态（原型 §1.3）。
}

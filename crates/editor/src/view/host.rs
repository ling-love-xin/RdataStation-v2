//! 编辑器宿主面板（一个面板 = 一个标签 = 一份文档）
//!
//! 标签条由 Dock 提供（架构 D13 / P0.1 结论）：面板只负责标题（文档名）、`title_suffix`（脏点）
//! 与正文（编辑内核）。**不自绘标签条**——V1 自绘 35px 标签条 + 溢出菜单花了约 300 行，换来的是
//! 与 Dock 更差的一致性（拖拽排序 / 键盘导航 / 溢出都要自己再实现一遍）。
//!
//! 多文档 = 同一个 tab 组里的多个面板：`DockLayout::tabs()` 里逐个 `panel_view` 即可。

use std::cell::RefCell;

use gpui_kit::base::StyledExt as _;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::button::{Button, ButtonVariants as _, DropdownButton};
use gpui_kit::component::dock::{
    BasePanel, DockArea, Panel as ComponentPanel, PanelEvent as BasePanelEvent, PanelId, TabGroup,
};
use gpui_kit::component::input::{Editor, EditorState, InputEvent};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::table::TableState;
use gpui_kit::component::Sizable as _;
use gpui_kit::*;

use crate::commands::{ExecuteAll, ExecuteSql, SaveDocument, ToggleComment};
use crate::edit;
use crate::execution::{self, ExecMenuKind, ExecTarget, ResultPlacement};
use crate::mode::{self, CellGranularity};
use crate::model::{DocumentId, EditorMode};
use crate::persist;
use crate::service::Document;
use crate::shared::EditorShared;
use crate::store::ResultEntry;
use crate::ui;
use crate::view::dialogs;
use crate::view::highlight;
use crate::view::widgets::result_grid::{self, ResultGridDelegate};
use crate::view::widgets::result_sets::{self, ResultSetTab};
use crate::view::widgets::status_bar::{self, StatusInputs};

/// 一份文档的编辑面板
pub struct EditorHostPanel {
    shared: EditorShared,
    document: DocumentId,
    focus_handle: FocusHandle,
    /// 编辑内核状态（构造需要 `window`，故在 `new` 里一次建好）
    editor: Entity<EditorState>,
    /// 语句数缓存（A7）：内容变化时算一次，**不在渲染期扫描**（render 是纯读路径）
    statements: usize,
    /// 所在的 tab 组（`Panel::on_added_to` 交给面板；用于“激活自己”）
    ///
    /// 面板不持有 `DockArea`：那是容器，容器知道面板就够了。反向只握一个弱句柄，
    /// 容器消失时自动失效（不会把整棵 Dock 拖住不放）。
    pub(crate) group: Option<WeakEntity<TabGroup>>,
    /// 状态栏提示：动作失败的真实原因（未命名需另存为 / 有未保存改动 / 只读拒绝）
    ///
    /// 有它才不会有“按了没反应”的动作：失败必须留下可见痕迹。成功即清空。
    pub(crate) message: Option<String>,
    /// 是否已被 Dock 移除（移除即关闭文档，见 `Panel::on_removed`）
    pub(crate) closed: bool,
    /// 结果网格（A14）：数据由 `ResultStore` 拷入 delegate，网格不持有第二份真值
    grid: Entity<TableState<ResultGridDelegate>>,
    /// 本文档**已提交、尚未回填**的语句数（B2：批量 = 多条；`> 0` 就是“执行中”）
    pending: usize,
    /// 本文档当前这轮执行是什么时候开始的（B3：状态栏耗时累加；`pending == 0` 时清掉）
    running_since: Option<std::time::Instant>,
    /// 结果区状态行文案（`Some` = 本文档有结果要显示）
    ///
    /// 缓存在这里而不是每帧 `format!`：`summary()` 要算，而 render 是纯读路径。
    result_summary: Option<String>,
    /// 结果集标签（从 `ResultStore` 投影；同样不在渲染期重算）
    result_tabs: Vec<ResultSetTab>,
    /// 当前选中的结果集下标（标签条的选中态）
    result_active: usize,
    /// 结果轮询任务句柄（空闲即退出；句柄存活到下一次提交）
    exec_pump: RefCell<Option<Task<()>>>,
    /// 内容回写订阅：句柄即生命周期（释放即取消）
    _editor_sub: Option<Subscription>,
}

impl EditorHostPanel {
    /// 为某文档建面板
    ///
    /// 文档不存在时退化为**只读空面板**而不 panic：面板可能在文档被关闭后仍被保留一帧，
    /// 让整个工作台跟着崩掉不是可接受的降级。
    pub fn new(
        shared: EditorShared,
        document: DocumentId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus_handle = cx.focus_handle();

        let (content, editor_read_only, mode) = {
            let service = shared.service();
            match service.find(&document) {
                Some(doc) => (
                    doc.content().to_string(),
                    !doc.read_only().can_edit(),
                    doc.mode(),
                ),
                None => (String::new(), true, EditorMode::Text),
            }
        };

        let editor = cx.new(|cx| {
            let mut state = EditorState::new(window, cx);
            state.set_value(content, window, cx);
            state.set_readonly(editor_read_only, cx);
            // A4：SQL 语义着色（文本模式是纯记事本，不解析不上色；分析模式留 1c 逐单元处理）
            if highlight::is_enabled(mode) {
                highlight::install(&mut state);
            }
            state
        });

        let sub = cx.subscribe_in(
            &editor,
            window,
            |this, _state, event: &InputEvent, _window, cx| {
                if matches!(event, InputEvent::Change) {
                    // 内核 → 服务：内容只落到 `EditorService`，脏状态由它比较基线得出
                    let text = this.editor_text(cx);
                    this.statements = count_statements(&text);
                    this.shared
                        .update(|service| service.set_content(&this.document, text));
                    // 重绘以刷新标签脏点与状态栏
                    cx.notify();
                }
            },
        );

        let statements = count_statements(
            &shared
                .service()
                .find(&document)
                .map(|doc| doc.content().to_string())
                .unwrap_or_default(),
        );

        // 结果网格：构造时一次建成，以后结果变化只 `refresh`（不重建，避免丢掉列宽 / 滚动位置）
        let grid = result_grid::new_table_state(
            ResultGridDelegate::empty("尚未执行——按 Ctrl+Enter 执行当前语句"),
            window,
            cx,
        );

        Self {
            shared,
            document,
            focus_handle,
            editor,
            statements,
            group: None,
            message: None,
            closed: false,
            grid,
            pending: 0,
            running_since: None,
            result_summary: None,
            result_tabs: Vec::new(),
            result_active: 0,
            exec_pump: RefCell::new(None),
            _editor_sub: Some(sub),
        }
    }

    pub fn document(&self) -> &DocumentId {
        &self.document
    }

    /// 当前文档是否脏（与基线不同）
    pub fn is_dirty(&self) -> bool {
        self.with_document(Document::is_dirty).unwrap_or(false)
    }

    /// 面板是否已被 Dock 移除（文档随之关闭）
    ///
    /// 宿主用它清理自己那份面板列表：`DockArea` 关掉一个 tab 时不会通知宿主，
    /// 只有面板自己知道（`Panel::on_removed`）。
    pub fn is_closed(&self) -> bool {
        self.closed
    }

    /// 保存当前文档（**只从事件路径调用**：`Ctrl+S` / 保存命令）
    ///
    /// 先写盘、后清脏（见 `persist::save_document`）：写失败不会留下“已保存”的假状态。
    /// 未命名文档返回 `Untitled`，调用方应改走“另存为”。
    pub fn save(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Result<std::path::PathBuf, persist::PersistError> {
        let result = persist::save_document(&self.shared, &self.document);
        if result.is_ok() {
            cx.notify();
        }
        result
    }

    /// 另存为：写盘 → 换路径与标题（文档**身份不变**，只换路径）
    ///
    /// 路径由宿主给出（系统文件对话框在 workbench 侧，本 crate 不依赖 `rfd`）。
    pub fn save_as(
        &mut self,
        path: &std::path::Path,
        cx: &mut Context<Self>,
    ) -> Result<std::path::PathBuf, persist::PersistError> {
        let result = persist::save_as(&self.shared, &self.document, path);
        if result.is_ok() {
            cx.notify();
        }
        result
    }

    /// 文档标题（宿主拼确认文案用；文档不存在时给“（已关闭）”）
    pub fn title_text(&self) -> String {
        self.with_document(|doc| doc.title().to_string())
            .unwrap_or_else(|| "（已关闭）".to_string())
    }

    /// 文档落盘路径（另存为对话框的默认目录 / 默认文件名用它）
    pub fn document_path(&self) -> Option<std::path::PathBuf> {
        self.with_document(|doc| doc.path().map(std::path::Path::to_path_buf))
            .flatten()
    }

    /// 文档当前模式（宿主取另存为的默认文件名后缀用它）
    pub fn document_mode(&self) -> Option<EditorMode> {
        self.with_document(|doc| doc.mode())
    }

    /// 把自己所在的标签激活（宿主打开已存在文档时调用）
    ///
    /// `TabGroup::select_tab` 是 Dock 公开的激活入口；面板从 `on_added_to` 拿到组句柄，
    /// 因此不必反持 `DockArea`（容器知道面板就够了）。
    ///
    /// “当前文档”在这里**直接**置位，不等 Dock 的帧末激活回调：面板是组里唯一标签时
    /// `select_tab` 会因“已经是当前标签”而早退（不发 `ActiveChanged`），
    /// 但服务层的当前文档可能已经被别的打开动作抢走过。
    pub fn focus_self(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.shared
            .update(|service| service.activate(&self.document));
        let Some(group) = self.group.clone() else {
            return;
        };
        let id = PanelId::from(cx.entity_id());
        let found = group.update(cx, |group, cx| {
            let index = group
                .panels()
                .iter()
                .position(|panel| panel.panel_id(cx) == id);
            if let Some(index) = index {
                group.select_tab(index, window, cx);
            }
            index.is_some()
        });
        if !matches!(found, Ok(true)) {
            // 组已不在（面板被拖到另一个组 / 整棵 Dock 被重建）：动作不落地，但不静默
            self.set_message(Some("面板不在标签组里，无法激活".to_string()), cx);
        }
    }

    /// 状态栏提示（`None` = 清空）
    pub fn set_message(&mut self, text: Option<String>, cx: &mut Context<Self>) {
        self.message = text;
        cx.notify();
    }

    /// 清掉状态栏提示（动作成功 / 用户取消时调：旧提示不该继续挂着）
    pub fn clear_message(&mut self, cx: &mut Context<Self>) {
        self.set_message(None, cx);
    }

    /// 当前会话快照（关文档 / 退出时保存用）
    ///
    /// 未命名文档没有稳定标识，返回 `None`（不存——重启后无法把它认回来）。
    pub fn session_snapshot(&self, cx: &App) -> Option<crate::session::SavedSession> {
        let (path, mode) = self.with_document(|doc| {
            let path = doc.path()?.to_string_lossy().into_owned();
            Some((path, doc.mode()))
        })??;
        let id = crate::session::session_id_for_path(std::path::Path::new(&path));

        let (cursor, selection) = {
            let state = self.editor.read(cx);
            let range = state.selected_range();
            let selection = (range.end > range.start).then_some((range.start, range.end));
            (range.start, selection)
        };

        Some(crate::session::SavedSession {
            id,
            path: Some(path),
            mode,
            content: self.editor_text(cx),
            cursor,
            selection,
        })
    }

    /// 保存会话（关文档 / 退出时调用；未命名或空文档自动跳过）
    pub fn save_session_now(&self, cx: &App) {
        let Some(session) = self.session_snapshot(cx) else {
            return;
        };
        if !crate::session::worth_saving(&session) {
            return;
        }
        if let Err(error) = self.shared.save_session(&session) {
            // 事件路径上不弹窗：落到 stderr 由日志承接，不静默吞掉
            eprintln!("[editor] 会话保存失败（{}）：{error}", session.id);
        }
    }

    /// 恢复会话：把光标/选区写回内核（模式与内容由宿主开文档时给出）
    pub fn restore_session(
        &mut self,
        session: &crate::session::SavedSession,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // 会话可能比现在的文档长（内容被截断 / 外部修改）：光标钳到当前文本末尾
        let len = self.editor_text(cx).len();
        let clamp = |offset: usize| offset.min(len);
        let start = clamp(session.selection.map(|(start, _)| start).unwrap_or(session.cursor));
        let end = clamp(session.selection.map(|(_, end)| end).unwrap_or(session.cursor));

        self.editor.update(cx, |state, cx| {
            // `set_cursor_position` 是内核里唯一“滚到指定偏移”的公开入口（会顺带聚焦）
            let position = {
                use gpui_kit::component::input::RopeExt as _;
                state.text().offset_to_position(start)
            };
            state.set_cursor_position(position, window, cx);
            state.set_selected_range(start..end, cx);
        });
        cx.notify();
    }

    /// 编辑器是否可输入（供窗口测试断言只读组合；生产代码读同一个判据）
    pub fn is_editable_for_test(&self) -> bool {
        !self.editor_read_only()
    }

    /// 当前内核文本（供测试断言动作效果）
    pub fn text_for_test(&self, cx: &App) -> String {
        self.editor_text(cx)
    }

    /// 编辑内核的焦点句柄（供测试把焦点交给内核，模拟真实打字场景）
    pub fn editor_focus_handle_for_test(&self, cx: &App) -> FocusHandle {
        self.editor.read(cx).focus_handle(cx)
    }

    /// 当前选区（供测试断言光标 / 选区恢复：无选区时是 `cursor..cursor`）
    pub fn selected_range_for_test(&self, cx: &App) -> std::ops::Range<usize> {
        self.editor.read(cx).selected_range()
    }

    /// 只读访问当前文档（渲染路径用；不克隆整份文档）
    fn with_document<R>(&self, read: impl FnOnce(&Document) -> R) -> Option<R> {
        let service = self.shared.service();
        service.find(&self.document).map(read)
    }

    fn editor_text(&self, cx: &App) -> String {
        self.editor.read(cx).value().to_string()
    }

    /// 编辑内核快照（文本 + 选区）：执行目标解析的输入
    fn editor_snapshot(&self, cx: &App) -> (String, std::ops::Range<usize>) {
        let state = self.editor.read(cx);
        (state.value().to_string(), state.selected_range())
    }

    /// 工具栏（原型 §2.2）：按其分层只放**今天真有动作**的控件
    ///
    /// - 最左：**模式指示器**（文本 / SQL / 分析，点击切换）
    /// - 执行级：**执行 ▾**——主按钮 = 执行（选区优先 → 当前语句），下拉 = 执行族里已经能跑的
    ///   三项（当前语句 / 选区 / 全部）。**只在 SQL 模式出现**：文本模式按能力表不通信，
    ///   分析模式的执行属笔记级动作（1c 随单元落地）。
    /// - 还没实现的（格式化 / 历史 / ⋯更多 / 执行位置 / 连接）**不放按钮**——
    ///   “只宣传不实现”是原型 §2.2 的明确排除项。
    fn render_toolbar(&self, mode: EditorMode, cx: &mut Context<Self>) -> impl IntoElement {
        let border = cx.theme().colors.border;
        let entity = cx.entity();

        let mut toolbar = div()
            .h_flex()
            .items_center()
            .gap_1()
            .px_2()
            .h(rems(ui::EDITOR_TOOLBAR_HEIGHT))
            .border_b(ui::HAIRLINE)
            .border_color(border)
            .child(
                Button::new("editor-mode-indicator")
                    .ghost()
                    .small()
                    // 测试按选择器断言“这个模式该有哪些控件”：模式与工具栏分层不是装饰
                    .debug_selector(|| "editor-mode-indicator".to_string())
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

        if mode == EditorMode::Sql {
            // 只有 SQL 模式的执行是“对当前文档执行”：文本模式按能力表不通信，
            // 分析模式的执行是**笔记级**动作（单元级按钮 + Shift+Enter，1c 落地）
            toolbar = toolbar.child(self.render_exec_group(cx));
            // 连接选择器（通道级 / 连接，右对齐）：原型 §2.2 把它放工具栏右侧
            toolbar = toolbar.child(div().ml_auto().child(self.render_connection_picker(cx)));
        }
        toolbar
    }

    /// 连接选择器（工具栏右侧）：本文档执行时用哪个连接（B1）
    ///
    /// - 已绑定 → 菜单里打勾；未绑定 → 菜单第一项「跟随当前连接」打勾（1a 口径，如实说明）
    /// - 选中一个连接时**先自动建连**（同 M4）：建连失败就**不绑定**，并在状态栏给出原因
    /// - 未接端口（宿主没给列表）时菜单直说“未接入连接列表”，不假装有可选项
    fn render_connection_picker(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity();
        let options = self.shared.connection_options();
        let has_port = self.shared.has_connections();
        let bound = self
            .with_document(|doc| doc.connection().map(str::to_string))
            .flatten();
        let label = match self.shared.connection_chip(bound.as_deref()) {
            Some(chip) => format!("{} ▾", chip.text()),
            None => "未绑定连接 ▾".to_string(),
        };
        let current = bound.clone();

        Button::new("editor-connection")
            .ghost()
            .small()
            .debug_selector(|| "editor-connection".to_string())
            .label(label)
            .dropdown_menu(move |menu, _window, _cx| {
                let mut menu = menu;
                if !has_port {
                    return menu.item(PopupMenuItem::new("（未接入连接列表）").disabled(true));
                }
                // 不绑定也是合法选择：执行跟随当前活动连接（与 1a 一致）
                let follow_entity = entity.clone();
                menu = menu.item(
                    PopupMenuItem::new("跟随当前连接")
                        .checked(current.is_none())
                        .on_click(move |_, _window, app| {
                            follow_entity.update(app, |panel, cx| {
                                panel.bind_connection(None, cx);
                            });
                        }),
                );
                menu = menu.separator();
                for option in options.iter() {
                    let is_current = current.as_deref() == Some(option.id.as_str());
                    let entity = entity.clone();
                    let label = format!("{} · {}", option.short, option.name);
                    let id = option.id.clone();
                    menu = menu.item(
                        PopupMenuItem::new(label)
                            .checked(is_current)
                            .on_click(move |_, _window, app| {
                                let id = id.clone();
                                entity.update(app, |panel, cx| {
                                    panel.bind_connection(Some(id), cx);
                                });
                            }),
                    );
                }
                menu
            })
    }

    /// 绑定连接（工具栏选择器调用）：先用端口确保已建连，失败就**不绑定**并留原因
    ///
    /// `None` = 解绑（跟随当前连接）。绑定是**文档属性**，同一窗口的多份文档互不影响。
    pub(crate) fn bind_connection(
        &mut self,
        conn_id: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let Some(conn_id) = conn_id else {
            self.shared
                .update(|service| service.set_connection(&self.document, None));
            self.set_message(None, cx);
            return;
        };

        match self.shared.ensure_connected(&conn_id) {
            Ok(()) => {
                self.shared.update(|service| {
                    service.set_connection(&self.document, Some(conn_id.clone()));
                });
                let chip = self.shared.connection_status_text(Some(&conn_id));
                self.set_message(Some(format!("已绑定 {chip}")), cx);
            }
            Err(reason) => {
                // 建连失败不绑定：半绑定状态比不绑定更难排查
                self.set_message(Some(format!("连接不可用：{reason}")), cx);
            }
        }
    }

    /// 执行族（执行级）：主按钮 + 下拉菜单
    ///
    /// 主按钮与 `Ctrl+Enter` 走**同一条路**（`resolve_target`：选区优先）；下拉里的五项是
    /// **显式目标**（`target_for_menu`，后两项见 [`ExecMenuKind`] 的注释）——“执行选区”
    /// 没选区时置灰、“批量执行”语句不足两条时置灰，比“点了没反应”明确。
    fn render_exec_group(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity();
        let run_entity = cx.entity();
        let (_, selection) = self.editor_snapshot(cx);
        let statements = self.statements;

        DropdownButton::new("editor-exec")
            .small()
            .button(
                Button::new("editor-exec-run")
                    .primary()
                    .small()
                    .debug_selector(|| "editor-exec-run".to_string())
                    .label("执行")
                    .on_click(move |_, _window, app| {
                        run_entity.update(app, |panel, cx| {
                            panel.execute_preferring_selection(cx);
                        });
                    }),
            )
            .dropdown_menu(move |menu, _window, _cx| {
                let mut menu = menu;
                for kind in ExecMenuKind::ALL {
                    let entity = entity.clone();
                    let available = kind.is_available(&selection, statements);
                    menu = menu.item(
                        PopupMenuItem::new(kind.label()).disabled(!available).on_click(
                            move |_, _window, app| {
                                entity.update(app, |panel, cx| {
                                    let (text, selection) = panel.editor_snapshot(cx);
                                    let target = execution::target_for_menu(kind, &text, selection);
                                    panel.execute(target, kind.placement(), cx);
                                });
                            },
                        ),
                    );
                }
                menu
            })
    }

    fn editor_read_only(&self) -> bool {
        self.with_document(|doc| !doc.read_only().can_edit())
            .unwrap_or(true)
    }

    /// 模式切换后同步视图侧状态（高亮是否启用、只读）
    ///
    /// 模式是**文档属性**：判定与确认在 `mode::plan_switch` 与对话框层，这里只负责
    /// “按当前模式刷新视图”，由切换流程在确认之后调用（**视图不自己改模式**）。
    pub fn sync_mode(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let Some(mode) = self.with_document(|doc| doc.mode()) else {
            return;
        };
        let read_only = self.editor_read_only();

        self.editor.update(cx, |state, cx| {
            state.set_readonly(read_only, cx);
            if highlight::is_enabled(mode) {
                highlight::install(state);
            } else {
                // 非 SQL 模式去掉着色（已缓存的 token 会随内容变化失效）
                state.lsp_mut().semantic_tokens_provider = None;
            }
        });
        cx.notify();
    }

    /// 把文档内容推回内核（另存为 / 外部修改后重新加载时调用）
    pub fn reload_from_document(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(text) = self.with_document(|doc| doc.content().to_string()) else {
            return;
        };
        self.statements = count_statements(&text);
        self.editor.update(cx, |state, cx| {
            if state.value() != text {
                state.set_value(text, window, cx);
            }
        });
        cx.notify();
    }

    // ===== 动作（A10）=====
    //
    // 动作只改共享状态或向内核写文本，不直接操作 Dock（关闭走 TabGroup 的请求路径）。

    /// `Ctrl+S`：保存；未命名 / 写盘失败 → 状态栏给出原因
    pub(crate) fn on_save(&mut self, _: &SaveDocument, _window: &mut Window, cx: &mut Context<Self>) {
        match self.save(cx) {
            Ok(_) => {
                // A12：保存时顺便把会话（光标/选区/模式）落库——用户按 Ctrl+S 是最自然的时机
                self.save_session_now(cx);
                self.set_message(None, cx);
            }
            Err(error) => self.set_message(Some(error.to_string()), cx),
        }
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

    // ===== 模式切换（A5 的矩阵 + A9 的确认对话框）=====
    //
    // 入口是工具栏最左的**模式指示器**（原型 §2.2）：点击选目标模式，代价由 `mode::plan_switch`
    // 判定，需确认的先弹对话框；确认后本面板落地（写内容 + 改模式 + 刷新着色/只读）。
    // **禁止静默切换**：任何需要付出代价的切换都不过状态（原型 §1.3）。

    /// 请求切换到目标模式（工具栏模式指示器调用）
    pub(crate) fn request_mode_switch(
        &mut self,
        to: EditorMode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(from) = self.with_document(|doc| doc.mode()) else {
            return;
        };
        if from == to {
            return;
        }

        let plan = self.switch_plan(from, to, CellGranularity::default(), cx);
        let Some(kind) = plan.confirm else {
            // 免确认（文本 → SQL）：直接切
            self.apply_mode_switch(plan, window, cx);
            return;
        };

        let entity = cx.entity();
        dialogs::open_switch_confirm(
            window,
            cx,
            kind,
            from,
            to,
            plan.note,
            move |granularity, window, cx| {
                entity.update(cx, |panel, cx| {
                    panel.confirm_mode_switch(to, granularity, window, cx);
                });
            },
        );
    }

    /// 确认后的落地（对话框回调调用）
    ///
    /// “从哪个模式切过来”在这里**重新读**而不是用弹窗前的快照：粒度是确认时才拿到的输入，
    /// 计划必须带着它重算（拿弹窗前那份就等于丢掉用户的选择）。
    pub fn confirm_mode_switch(
        &mut self,
        to: EditorMode,
        granularity: CellGranularity,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(from) = self.with_document(|doc| doc.mode()) else {
            return;
        };
        let plan = self.switch_plan(from, to, granularity, cx);
        self.apply_mode_switch(plan, window, cx);
    }

    /// 算一份切换计划（读取当前文本与“有没有结果”，交给纯函数判定）
    fn switch_plan(
        &self,
        from: EditorMode,
        to: EditorMode,
        granularity: CellGranularity,
        cx: &App,
    ) -> mode::SwitchPlan {
        mode::plan_switch(
            from,
            to,
            &self.editor_text(cx),
            self.result_summary.is_some(),
            granularity,
        )
    }

    /// 落地一次模式切换（**已确认**）：内容变换 → 模式变更 → 视图刷新
    ///
    /// 内容先写回 `EditorService`，再让面板从文档重载（`reload_from_document`）——
    /// 内容只有一条真值路径，不直接把变换结果塞进内核（否则服务层与内核就分家了）。
    fn apply_mode_switch(
        &mut self,
        plan: mode::SwitchPlan,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let rewritten = match plan.content {
            mode::SwitchContent::Unchanged => None,
            mode::SwitchContent::Sql(text) => Some(text),
            mode::SwitchContent::Cells(cells) => Some(mode::cells_to_text(&cells)),
        };
        if let Some(text) = rewritten {
            let statements = count_statements(&text);
            self.shared
                .update(|service| service.set_content(&self.document, text));
            self.statements = statements;
        }
        self.shared
            .update(|service| service.set_mode(&self.document, plan.to));
        self.sync_mode(window, cx);
        self.reload_from_document(window, cx);
        self.set_message(plan.note.map(str::to_string), cx);
    }

    // ===== 执行（A14）=====
    //
    // 四个入口（Ctrl+Enter / Ctrl+Shift+Enter / 工具栏主按钮 / 执行族菜单）共用 `execute`：
    // 目标解析 → 提交 → 轮询回填。界面不等 I/O。

    /// `Ctrl+Enter`：执行（选区优先 → 光标所在语句）
    pub(crate) fn on_execute_sql(
        &mut self,
        _: &ExecuteSql,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.execute_preferring_selection(cx);
    }

    /// 执行（选区优先 → 当前语句）：快捷键与工具栏主按钮共用同一条路
    pub(crate) fn execute_preferring_selection(&mut self, cx: &mut Context<Self>) {
        let (text, selection) = self.editor_snapshot(cx);
        let target = execution::resolve_target(&text, selection);
        self.execute(target, ResultPlacement::Replace, cx);
    }

    /// `Ctrl+Shift+Enter`：执行全部
    pub(crate) fn on_execute_all(
        &mut self,
        _: &ExecuteAll,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.run_all(cx);
    }

    /// 执行整篇（**宿主发起的唯一入口**：导航「查看数据」打开文档后自动跑一次，B11）
    ///
    /// 与 `Ctrl+Shift+Enter` 同一条路：目标解析 → 提交 → 轮询回填。
    pub fn run_all(&mut self, cx: &mut Context<Self>) {
        let (text, _selection) = self.editor_snapshot(cx);
        let target = execution::all_target(&text);
        self.execute(target, ResultPlacement::Replace, cx);
    }

    /// 提交一次执行
    ///
    /// `placement` 决定结果落到当前结果集还是新结果集（B2 的「在新结果标签中执行」/「批量执行」）。
    /// 拒绍都要留痕迹：文本模式（能力表禁止通信）、未接入执行、忙、空目标。
    pub(crate) fn execute(
        &mut self,
        target: ExecTarget,
        placement: ResultPlacement,
        cx: &mut Context<Self>,
    ) {
        if !self.execution_allowed() {
            self.set_message(Some("文本模式不与数据库通信".to_string()), cx);
            return;
        }
        // 预期回填条数 = 本次要跑的语句数（批量 > 1）：跑完这几条才算“不执行中”
        let expected = target.statements().len();
        match self.shared.submit(self.document.clone(), &target, placement) {
            Ok(()) => {
                if self.pending == 0 {
                    // 批量期间一直计同一轮的时间：后一句不该把计时清零
                    self.running_since = Some(std::time::Instant::now());
                }
                self.pending += expected;
                self.set_message(None, cx);
                self.ensure_exec_pump(cx);
            }
            Err(error) => self.set_message(Some(error.message().to_string()), cx),
        }
    }

    /// 中断当前执行（B3；原型 §5.1：入口在**状态栏 ■**）
    ///
    /// 同步只回绍“没在跑”；真正的中断在工作线程上做（端口实现允许阻塞）。
    /// 中断之后的剩余语句由通道标“已取消”（不是接着往下跑）。
    pub(crate) fn interrupt(&mut self, cx: &mut Context<Self>) {
        match self.shared.cancel() {
            Ok(()) => self.set_message(Some("已请求中断…".to_string()), cx),
            Err(reason) => self.set_message(Some(reason), cx),
        }
    }

    /// 本文档现在能不能中断（状态栏据此决定要不要画■按钮）
    fn can_interrupt(&self) -> bool {
        self.pending > 0
    }

    /// 状态栏那行“执行中 3.4s…”读的耗时（供测试断言“跑完就不再计时”）
    pub fn elapsed_for_test(&self) -> Option<std::time::Duration> {
        self.running_since.map(|since| since.elapsed())
    }

    /// 当前模式是否允许执行（**读能力表**，不在视图里另写一份模式判断）
    ///
    /// 文本模式恒为 `false`（“不与数据库通信”是能力表里的硬约束）。
    fn execution_allowed(&self) -> bool {
        self.with_document(|doc| doc.capabilities().execute)
            .unwrap_or(false)
    }

    /// 本文档的文件档位提示（>50MB / ≥200MB；`None` = 不显示提示卡）
    pub fn tier_notice(&self) -> Option<&'static str> {
        self.with_document(|doc| doc.tier_notice()).flatten()
    }

    /// 本文档的执行状态与结果摘要（供测试断言；`None` = 尚未执行）
    pub fn result_summary_for_test(&self) -> Option<&str> {
        self.result_summary.as_deref()
    }

    /// 结果集标签（供测试断言“批量跑出三个结果集”与标签文案）
    pub fn result_tabs_for_test(&self) -> Vec<(String, bool)> {
        self.result_tabs
            .iter()
            .map(|tab| (tab.label.clone(), tab.failed))
            .collect()
    }

    /// 当前选中的结果集下标（供测试断言点标签真的切了）
    pub fn result_active_for_test(&self) -> usize {
        self.result_active
    }

    /// 还有几条语句没回填（供测试断言“批量执行至少知道自己在跑什么”）
    pub fn pending_for_test(&self) -> usize {
        self.pending
    }

    /// 结果网格当前行数（供测试断言网格真的拿到了数据）
    pub fn grid_row_count_for_test(&self, cx: &App) -> usize {
        self.grid.read(cx).delegate().row_count_for_test()
    }

    /// 把光标放到指定位移（供测试断言“执行的是光标所在那句”；键位路径仍走真按键）
    pub fn set_caret_for_test(&mut self, offset: usize, cx: &mut Context<Self>) {
        self.editor
            .update(cx, |state, cx| state.set_selected_range(offset..offset, cx));
    }

    /// 启动结果轮询（已有存活任务时不重复启动）
    ///
    /// 与 `workbench` 的导航任务同一模式：**后台等、主线程回填**，空闲即退出。
    /// 执行期间顺带 `cx.notify()`：状态栏的耗时累加跟着这个节拍走（不另起一个定时器）。
    fn ensure_exec_pump(&self, cx: &mut Context<Self>) {
        if let Some(task) = self.exec_pump.borrow().as_ref()
            && !task.is_ready()
        {
            return;
        }
        let weak = cx.entity().downgrade();
        let executor = cx.background_executor().clone();
        let task = cx.spawn(async move |_this, cx| {
            loop {
                executor.timer(std::time::Duration::from_millis(60)).await;
                // 还有执行在跑（含别的文档）就继续等；一个都没有了就退出，下次提交重新起
                let keep_going = weak
                    .update(cx, |this, cx| {
                        this.drain_exec_results(cx);
                        if this.pending > 0 {
                            cx.notify();
                        }
                        this.shared.is_executing()
                    })
                    .unwrap_or(false);
                if !keep_going {
                    return;
                }
            }
        });
        *self.exec_pump.borrow_mut() = Some(task);
    }

    /// 取回已完成的执行，填入结果存储与网格
    ///
    /// **一条语句一条结论**：批量执行会连续回来多条，每条各自落一个结果集（落位由 outcome
    /// 自带）。本文档的回填计数减到 0 才算执行完。
    pub(crate) fn drain_exec_results(&mut self, cx: &mut Context<Self>) {
        // 中断尝试的结果先收（失败 / 没在跑 都要留痕，不能默默把按钮点一下就算完）
        if let Some(note) = self.shared.drain_cancel_notes().into_iter().last() {
            self.set_message(Some(note), cx);
        }

        let outcomes = self.shared.drain_exec();
        let mut mine_arrived = 0usize;
        for outcome in outcomes {
            let is_mine = outcome.document == self.document;
            if is_mine {
                mine_arrived += 1;
            }
            let placement = outcome.placement;
            let entry = entry_from(outcome);
            self.shared.update_results(|store| store.push(entry, placement));
        }
        if mine_arrived > 0 {
            self.pending = self.pending.saturating_sub(mine_arrived);
            if self.pending == 0 {
                self.running_since = None;
            }
            self.sync_result_view(cx);
        }
    }

    /// 按「当前选中的结果集」刷新结果区（网格数据 + 状态行 + 标签条）
    ///
    /// **结果区唯一的读点**：结果集可能因一次执行回填、点标签、批量推进而变，但界面只从
    /// 这里读一次——否则会出现“网格还是上一份、摘要已经换了”的错位（多结果集之后这种错位
    /// 很难靠看界面发现）。
    fn sync_result_view(&mut self, cx: &mut Context<Self>) {
        // 先把要用的数据从权威存储里拷出来（不把 `Ref` 带进下面的 `grid.update`）
        let (tabs, active, grid_data, failed_text, summary, error) = {
            let store = self.shared.results();
            let active = store.active_index(&self.document).unwrap_or(0);
            let entry = store.active(&self.document);
            (
                result_sets::tabs(store.sets(&self.document)),
                active,
                entry
                    .filter(|entry| entry.has_grid())
                    .map(|entry| (entry.columns.clone(), entry.rows.clone())),
                entry
                    .filter(|entry| !entry.has_grid())
                    .map(ResultEntry::summary),
                entry.map(ResultEntry::summary),
                entry.and_then(|entry| entry.error.clone()),
            )
        };

        self.result_tabs = tabs;
        self.result_active = active;
        self.result_summary = summary;
        self.grid.update(cx, |state, cx| {
            match grid_data {
                Some((columns, rows)) => state.delegate_mut().set_data(columns, rows),
                None => state.delegate_mut().clear(failed_text.unwrap_or_default()),
            }
            state.refresh(cx);
        });

        // 失败原因同时进状态栏（结果区可能被滚出视野）；选中成功的那份则清掉旧提示
        self.set_message(error, cx);
    }

    /// 切换结果集（结果集标签条点击）：选中项只有 `ResultStore` 能改，界面按它重画
    pub(crate) fn select_result_set(&mut self, index: usize, cx: &mut Context<Self>) {
        let changed = self
            .shared
            .update_results(|store| store.select(&self.document, index));
        if changed {
            self.sync_result_view(cx);
        }
    }

    /// 是否要显示结果区（本文档有结果或正在执行；且模式允许通信）
    fn result_visible(&self) -> bool {
        self.execution_allowed() && (self.pending > 0 || self.result_summary.is_some())
    }

    /// `Ctrl+W`：请求关闭当前文档
    ///
    /// **不由面板自己执行**：`DockArea` 移除面板会读面板本体（可见性 / 可关闭性），
    /// 从面板自己的 `update`（动作处理器就在其中）里发起就是重入，GPUI 直接 panic。
    /// 所以关闭由宿主调用 [`close_document_in_dock`]，面板只负责回答“能不能关”。
    /// 脏文档在这里拦下并说明原因（Dock 没有“关闭前否决”钩子，架构 §12 #18）；
    /// 三态确认（保存 / 不保存 / 取消）由宿主弹 [`dialogs::open_close_confirm`]。
    pub(crate) fn refuse_close_when_dirty(&mut self, cx: &mut Context<Self>) -> bool {
        if !self.is_dirty() {
            return false;
        }
        self.set_message(Some("有未保存的改动，先保存再关闭".to_string()), cx);
        true
    }
}

impl EventEmitter<BasePanelEvent> for EditorHostPanel {}

/// 执行结论 → 结果记录（视图模型转换，不在这做任何 I/O）
fn entry_from(outcome: execution::ExecOutcome) -> ResultEntry {
    match outcome.result {
        Ok(data) => ResultEntry::success(
            outcome.document,
            outcome.sql,
            data.elapsed_ms,
            data.truncated,
            data.columns,
            data.rows,
        ),
        Err(error) => ResultEntry::failure(outcome.document, outcome.sql, error, 0),
    }
}

/// 关闭一份文档的面板（**宿主调用**，比如 `Ctrl+W`）
///
/// 为什么是宿主的活：[`Panel::on_removed`] 会回读面板（可见性 / 可关闭性），
/// 而动作处理器本身就在面板的 `update` 里——从那里发起关闭是重入，GPUI 会直接 panic。
/// 宿主不在面板的 `update` 中，所以只有它能安全地让容器移除面板。
///
/// **脏文档在这里被拦下**（返回 `false`，并在面板状态栏留原因）：Dock 没有“关闭前否决”
/// 钩子（架构 §12 #18），这个判据就是安全网。要真正关掉脏文档，调用方必须先走三态确认
/// （[`dialogs::open_close_confirm`]）再调 [`close_document_now`]。
///
/// 文档本身由 [`EditorHostPanel::on_removed`] 从 `EditorService` 里关闭——
/// “一个面板 = 一份文档”的对应关系只有一处。
pub fn close_document_in_dock(
    area: &Entity<DockArea>,
    panel: Entity<EditorHostPanel>,
    window: &mut Window,
    cx: &mut App,
) -> bool {
    if panel.read(cx).is_dirty() {
        panel.update(cx, |panel, cx| panel.refuse_close_when_dirty(cx));
        return false;
    }
    close_document_now(area, panel, window, cx)
}

/// 直接关闭（**不做脏检查**）：调用方已就未保存改动拿到用户选择
///
/// 只应有两条调用路径：文档本来就干净（走 [`close_document_in_dock`]）、
/// 用户在关闭确认里选了“不保存”（走 [`request_close_document`]）。
pub fn close_document_now(
    area: &Entity<DockArea>,
    panel: Entity<EditorHostPanel>,
    window: &mut Window,
    cx: &mut App,
) -> bool {
    area.update(cx, |area, cx| area.remove_panel(panel, window, cx));
    true
}

/// 关闭请求的完整流程（宿主键位 / 标签关闭都走它）
///
/// 干净 → 直接关；脏 → 三态确认（保存 / 不保存 / 取消）。流程放在这里而不是宿主，
/// 是因为它是**编辑器语义**（写盘、二次确认、失败分支）——宿主只负责“把面板从 Dock 上摘掉”
/// 与“弹系统文件对话框”（路径选择端口注入）。
pub fn request_close_document(
    area: &Entity<DockArea>,
    panel: Entity<EditorHostPanel>,
    window: &mut Window,
    cx: &mut App,
) {
    if !panel.read(cx).is_dirty() {
        close_document_in_dock(area, panel, window, cx);
        return;
    }

    let title = panel.read(cx).title_text();
    let dialog_panel = panel.clone();
    let dialog_area = area.clone();
    dialogs::open_close_confirm(window, cx, title, move |choice, window, cx| {
        resolve_close_choice(&dialog_area, dialog_panel.clone(), choice, window, cx);
    });
}

/// 用户在“关闭前确认”上的选择落地（对话框回调与测试都走这里）
///
/// 与模式切换的 `confirm_mode_switch` 同一口径：弹窗只负责收集选择，
/// 落地只有一个入口——否则“对话框里点了保存”与“测试里走保存分支”会变成两段代码。
pub fn resolve_close_choice(
    area: &Entity<DockArea>,
    panel: Entity<EditorHostPanel>,
    choice: dialogs::CloseChoice,
    window: &mut Window,
    cx: &mut App,
) {
    match choice {
        // 取消：什么都不做（小改动常来自误触关闭，不该顺手丢掉）
        dialogs::CloseChoice::Cancel => {}
        dialogs::CloseChoice::Discard => {
            close_document_now(area, panel, window, cx);
        }
        dialogs::CloseChoice::Save => {
            save_then_close(area.clone(), panel, window, cx);
        }
    }
}

/// 保存当前文档（`Ctrl+Shift+S`，**不关文档**）：选路径 → 写盘；返回是否真写盘
///
/// 失败只落到状态栏（这里没在关闭流程上，不该用弹框拦住用户的手）。
pub fn request_save_as(panel: &Entity<EditorHostPanel>, cx: &mut App) -> bool {
    match pick_and_save_as(panel, cx) {
        Ok(()) => {
            panel.update(cx, |panel, cx| panel.clear_message(cx));
            true
        }
        Err(SaveAsFailure::Cancelled) => {
            // 用户取消：不是错误，但也不留上一次的提示
            panel.update(cx, |panel, cx| panel.clear_message(cx));
            false
        }
        Err(failure) => {
            let message = failure.message();
            panel.update(cx, |panel, cx| panel.set_message(Some(message), cx));
            false
        }
    }
}

/// 另存为的四种结局（调用方据此决定“关不关 / 弹不弹二次确认”）
///
/// “取消”单独成一类而不是 `None`：**用户取消不是失败**，不该弹二次确认、也不该报“未接入”。
enum SaveAsFailure {
    /// 宿主没接系统文件对话框（端口未注入）
    NoPicker,
    /// 用户取消
    Cancelled,
    /// 写盘失败（占用 / 权限 / 磁盘）
    Io(persist::PersistError),
    /// 文档已关闭（面板可能多活一帧）
    Gone,
}

impl SaveAsFailure {
    /// 状态栏文案（取消不产生文案，由调用方单独处理）
    fn message(&self) -> String {
        match self {
            Self::NoPicker => "未接入系统文件对话框，无法另存为".to_string(),
            Self::Cancelled => String::new(),
            Self::Io(error) => format!("另存为失败：{error}"),
            Self::Gone => "文档已关闭".to_string(),
        }
    }
}

impl From<persist::PersistError> for SaveAsFailure {
    fn from(error: persist::PersistError) -> Self {
        Self::Io(error)
    }
}

/// 另存为的“选路径 + 写盘”核心（不含失败后的交互）
///
/// 路径选择走宿主注入的端口（[`EditorShared::pick_save_path`]）：编辑器**不依赖** `rfd`。
fn pick_and_save_as(
    panel: &Entity<EditorHostPanel>,
    cx: &mut App,
) -> Result<(), SaveAsFailure> {
    let (current, mode) = {
        let read = panel.read(cx);
        (read.document_path(), read.document_mode())
    };
    let Some(mode) = mode else {
        return Err(SaveAsFailure::Gone);
    };
    if !panel.read(cx).shared.has_save_path_picker() {
        return Err(SaveAsFailure::NoPicker);
    }

    let default_name = mode.default_file_name().to_string();
    let picked = panel
        .read(cx)
        .shared
        .pick_save_path(current, default_name);
    let Some(path) = picked else {
        return Err(SaveAsFailure::Cancelled);
    };

    panel.update(cx, |panel, cx| panel.save_as(&path, cx)).map(|_| ())?;
    Ok(())
}

/// 保存后关闭（关闭三态的“保存”分支）
///
/// 三种结果分别处理：写盘成功 → 关；未命名 → 先另存为（用户取消就不关）；
/// 写盘失败（占用 / 权限 / 磁盘）→ 二次确认（重试 / 另存为 / 取消），
/// **绝不把脏文档当干净关掉**。
fn save_then_close(
    area: Entity<DockArea>,
    panel: Entity<EditorHostPanel>,
    window: &mut Window,
    cx: &mut App,
) {
    match panel.update(cx, |panel, cx| panel.save(cx)) {
        Ok(_) => {
            close_document_now(&area, panel, window, cx);
        }
        Err(persist::PersistError::Untitled(_)) => {
            // 未命名：没有路径可写，先选一个（取消就不关）
            pick_save_as_then_close(area, panel, window, cx);
        }
        Err(error) => {
            let title = panel.read(cx).title_text();
            let reason = error.to_string();
            open_save_failure(&area, &panel, title, reason, window, cx);
        }
    }
}

/// 另存为后关闭（“保存”分支里未命名文档 / 用户选“另存为”的路径）
fn pick_save_as_then_close(
    area: Entity<DockArea>,
    panel: Entity<EditorHostPanel>,
    window: &mut Window,
    cx: &mut App,
) {
    match pick_and_save_as(&panel, cx) {
        Ok(()) => {
            close_document_now(&area, panel, window, cx);
        }
        Err(SaveAsFailure::Cancelled) => {}
        Err(SaveAsFailure::Io(error)) => {
            let title = panel.read(cx).title_text();
            let reason = error.to_string();
            open_save_failure(&area, &panel, title, reason, window, cx);
        }
        Err(failure) => {
            let message = failure.message();
            panel.update(cx, |panel, cx| panel.set_message(Some(message), cx));
        }
    }
}

/// 写盘失败后的二次确认（重试 / 另存为 / 取消）
///
/// 三条路都保留：文件被占用 / 短暂无权限时“重试”最省事；目标路径本身不行时“另存为”能绕开；
/// “取消”后文档仍是脏的（真实状态不粉饰）。
fn open_save_failure(
    area: &Entity<DockArea>,
    panel: &Entity<EditorHostPanel>,
    title: String,
    reason: String,
    window: &mut Window,
    cx: &mut App,
) {
    let retry_area = area.clone();
    let retry_panel = panel.clone();
    let as_area = area.clone();
    let as_panel = panel.clone();
    let cancel_panel = panel.clone();
    dialogs::open_save_failure_confirm(
        window,
        cx,
        title,
        reason,
        move |choice, window, cx| match choice {
            dialogs::SaveFailureChoice::Cancel => {
                // 取消保存：文档仍是脏的（未保存状态是真实状态，不该粉饰）
                cancel_panel.update(cx, |panel, cx| panel.clear_message(cx));
            }
            dialogs::SaveFailureChoice::Retry => {
                save_then_close(retry_area.clone(), retry_panel.clone(), window, cx);
            }
            dialogs::SaveFailureChoice::SaveAs => {
                pick_save_as_then_close(as_area.clone(), as_panel.clone(), window, cx);
            }
        },
    );
}


/// 语句数：走 `engine::sql::split` 的**词法级**切分（不是 `split(';')`）
///
/// 只在内容变化时调一次（构造 / 输入回写 / 重新加载），渲染期只读缓存值。
fn count_statements(text: &str) -> usize {
    engine::sql::split_statements(text).len()
}

impl Focusable for EditorHostPanel {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl BasePanel for EditorHostPanel {
    fn panel_name(&self) -> &'static str {
        "editor"
    }

    /// 可关闭：**脏文档不给 ✕**（Dock 无“关闭前否决”钩子，架构 §12 #18）
    ///
    /// 标签 ✕ 由 Dock 自己处理：`DockArea` 收到 `TabGroupEvent::ClosePanel` 就直接移除面板，
    /// 没有地方能插一句“先问用户”。因此脏文档的关闭入口只留**显式的那一个**：
    /// `Ctrl+W` → [`request_close_document`]（保存 / 不保存 / 取消）。
    /// 保存后脏点消失，✕ 自然回来。
    fn closable(&self, _cx: &App) -> bool {
        !self.is_dirty()
    }

    /// 记下所在标签组的弱句柄：
    /// - 把自己激活（宿主打开已存在文档时）
    /// - 请求关闭（`Ctrl+W`）
    fn on_added_to(
        &mut self,
        group: WeakEntity<TabGroup>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
        self.group = Some(group);
    }

    /// 成为当前显示的标签 → 把焦点交给编辑内核，切过去就能直接打字
    ///
    /// 同时把“当前文档”告知服务层：宿主的 `Ctrl+W` 等文档级动作按它找面板。
    fn set_active(&mut self, active: bool, window: &mut Window, cx: &mut Context<Self>) {
        if active && !self.closed {
            self.shared
                .update(|service| service.activate(&self.document));
            self.editor.update(cx, |state, cx| state.focus(window, cx));
        }
    }

    /// 被 Dock 移除 = 这份文档的生命周期结面：**一个面板 = 一份文档**，
    /// 面板走人，文档随之从 `EditorService` 里关掉（宿主不必再维护对应关系）。
    fn on_removed(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        // A12：走人之前把会话（光标 / 选区 / 模式 / 内容）落库——此刻文档与内核都还在
        self.save_session_now(cx);
        self.closed = true;
        self.group = None;
        self.shared
            .update(|service| service.close(&self.document));
        // 文档关了，结果也不留（结果不跟着已关闭的文档挂着）
        self.shared
            .update_results(|store| store.clear(&self.document));
        cx.notify();
    }
}

impl ComponentPanel for EditorHostPanel {
    fn tab_name(&self, _cx: &App) -> Option<SharedString> {
        self.with_document(|doc| SharedString::from(doc.title().to_string()))
    }

    fn title(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().text_sm().child(self.title_text())
    }

    /// 脏点：Dock 没有"关闭前询问"钩子，脏状态必须**在标签上可见**（否则用户会误关）
    fn title_suffix(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement> {
        self.is_dirty().then(|| {
            div()
                .size(rems(ui::STATUS_DOT_SIZE))
                .rounded_full()
                .bg(cx.theme().colors.primary)
        })
    }
}

impl Render for EditorHostPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (bg, primary, muted) = {
            let theme = cx.theme();
            (
                theme.colors.background,
                theme.colors.primary,
                theme.colors.muted_foreground,
            )
        };
        let _ = (primary, muted);
        let read_only = self.editor_read_only();

        // 光标 / 选区：从内核读真实值（行号列号按 1 基显示）
        let (line, column, selected_chars) = {
            let state = self.editor.read(cx);
            let cursor = state.cursor_position();
            (
                cursor.line as usize + 1,
                cursor.character as usize + 1,
                state.selected_text().chars().count(),
            )
        };
        // 连接段只在会通信的模式出现（文本模式没有连接概念，不显示占位）
        let communicating = self
            .with_document(|doc| doc.capabilities().execute)
            .unwrap_or(false);
        let connection_text = if communicating {
            let bound = self
                .with_document(|doc| doc.connection().map(str::to_string))
                .flatten();
            Some(self.shared.connection_status_text(bound.as_deref()))
        } else {
            None
        };
        let status = StatusInputs {
            mode: self.with_document(|doc| doc.mode()).unwrap_or(EditorMode::Text),
            dirty: self.is_dirty(),
            read_only: self
                .with_document(|doc| doc.read_only())
                .unwrap_or_default(),
            statements: self.statements,
            line,
            column,
            selected_chars,
            message: self.message.as_deref(),
            executing: self.pending > 0,
            elapsed: self.running_since.map(|since| since.elapsed()),
            connection: connection_text.as_deref(),
        };

        // 中断（B3）：只在本文档真有语句没回填时出现（原型 §5.1：「中断」挂在状态栏■）
        let interrupt = self.can_interrupt().then(|| {
            let entity = cx.entity();
            Button::new("editor-interrupt")
                .ghost()
                .small()
                .debug_selector(|| "editor-interrupt".to_string())
                .label("■ 中断")
                .on_click(move |_, _window, app| {
                    entity.update(app, |panel, cx| panel.interrupt(cx));
                })
                .into_any_element()
        });

        // 结果区：有结果或正在执行时出现（否则不占位置——不显示空壳）
        let result_summary = match (self.result_visible(), &self.result_summary) {
            (true, Some(summary)) => Some(summary.clone()),
            (true, None) => Some("执行中…".to_string()),
            (false, _) => None,
        };
        // 结果集标签条：两份以上结果才画（一份结果不需要切换器）
        let result_tabs = (result_summary.is_some() && self.result_tabs.len() >= 2).then(|| {
            let entity = cx.entity();
            result_sets::render(
                &self.result_tabs,
                self.result_active,
                move |index, _window, app| {
                    let index = *index;
                    entity.update(app, |panel, cx| panel.select_result_set(index, cx));
                },
                cx,
            )
            .into_any_element()
        });

        let mut root = div()
            .v_flex()
            .size_full()
            .min_h_0()
            .bg(bg)
            // A10：键位都绑在这个 context 上（`crates/app` 注册）。
            // 焦点在内核里时内核的 `Input` context 更深、先被尝试；它没处理的键再冒泡到这里。
            // `track_focus` 不能省：没有它面板不在 dispatch path 上，快捷键就落不到动作（已踩）。
            .key_context("editor")
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::on_save))
            .on_action(cx.listener(Self::on_toggle_comment))
            .on_action(cx.listener(Self::on_execute_sql))
            .on_action(cx.listener(Self::on_execute_all));
        // 工具栏（②）：模式指示器在这里，模式不再是只能从状态栏读到的短标签
        let mode = self.with_document(|doc| doc.mode()).unwrap_or(EditorMode::Text);
        root = root.child(self.render_toolbar(mode, cx));
        // 提示卡（A13）：档位带来的限制要在界面上说清，而不是让用户自己撞上（“能编辑却改不了”）
        if let Some(notice) = self.tier_notice() {
            let theme = cx.theme();
            let warning = theme.colors.warning;
            let warning_fill = warning.opacity(0.18);
            root = root.child(
                div()
                    .h_flex()
                    .items_center()
                    .gap_1()
                    .px_2()
                    .py_1()
                    .text_xs()
                    .text_color(warning)
                    .bg(warning_fill)
                    .border_b(ui::HAIRLINE)
                    .border_color(warning)
                    .child(SharedString::from(notice)),
            );
        }
        root = root.child(
                div()
                    .flex_1()
                    .min_h_0()
                    // 局部内距走 Tailwind 尺度（8px）；结构尺寸才进 ui.rs 常量表
                    .px_2()
                    .child(
                        Editor::new(&self.editor)
                            .appearance(false)
                            .bordered(false)
                            .readonly(read_only)
                            .size_full(),
                    ),
            );
        // 结果区：有结果或正在执行时出现（否则不占位置——不显示空壳）
        if let Some(summary) = result_summary {
            root = root.child(result_grid::render(&self.grid, &summary, result_tabs, cx));
        }
        root.child(status_bar::render(&status, interrupt, cx))
    }
}

//! 编辑器宿主面板（一个面板 = 一个标签 = 一份文档）
//!
//! 标签条由 Dock 提供（架构 D13 / P0.1 结论）：面板只负责标题（文档名）、`title_suffix`（脏点）
//! 与正文（编辑内核）。**不自绘标签条**——V1 自绘 35px 标签条 + 溢出菜单花了约 300 行，换来的是
//! 与 Dock 更差的一致性（拖拽排序 / 键盘导航 / 溢出都要自己再实现一遍）。
//!
//! 多文档 = 同一个 tab 组里的多个面板：`DockLayout::tabs()` 里逐个 `panel_view` 即可。

use gpui_kit::base::StyledExt as _;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::dock::{
    BasePanel, DockArea, Panel as ComponentPanel, PanelEvent as BasePanelEvent, PanelId, TabGroup,
};
use gpui_kit::component::input::{Editor, EditorState, InputEvent};
use gpui_kit::*;

use crate::commands::{SaveDocument, ToggleComment};
use crate::edit;
use crate::model::{DocumentId, EditorMode};
use crate::persist;
use crate::service::Document;
use crate::shared::EditorShared;
use crate::ui;
use crate::view::highlight;
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

        Self {
            shared,
            document,
            focus_handle,
            editor,
            statements,
            group: None,
            message: None,
            closed: false,
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

    /// 编辑器是否可输入（供窗口测试断言只读组合；生产代码读同一个判据）
    pub fn is_editable_for_test(&self) -> bool {
        !self.editor_read_only()
    }

    /// 当前内核文本（供测试断言动作效果）
    pub fn text_for_test(&self, cx: &App) -> String {
        self.editor_text(cx)
    }

    /// 只读访问当前文档（渲染路径用；不克隆整份文档）
    fn with_document<R>(&self, read: impl FnOnce(&Document) -> R) -> Option<R> {
        let service = self.shared.service();
        service.find(&self.document).map(read)
    }

    fn editor_text(&self, cx: &App) -> String {
        self.editor.read(cx).value().to_string()
    }

    fn title_text(&self) -> String {
        self.with_document(|doc| doc.title().to_string())
            .unwrap_or_else(|| "（已关闭）".to_string())
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
            Ok(_) => self.set_message(None, cx),
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

    /// `Ctrl+W`：请求关闭当前文档
    ///
    /// **不由面板自己执行**：`DockArea` 移除面板会读面板本体（可见性 / 可关闭性），
    /// 从面板自己的 `update`（动作处理器就在其中）里发起就是重入，GPUI 直接 panic。
    /// 所以关闭由宿主调用 [`close_document_in_dock`]，面板只负责回答“能不能关”。
    /// 脏文档在这里拦下并说明原因（Dock 没有“关闭前否决”钩子，架构 §12 #18）；
    /// 三态确认（保存 / 不保存 / 取消）属对话框批次。
    pub(crate) fn refuse_close_when_dirty(&mut self, cx: &mut Context<Self>) -> bool {
        if !self.is_dirty() {
            return false;
        }
        self.set_message(Some("有未保存的改动，先保存再关闭".to_string()), cx);
        true
    }
}

impl EventEmitter<BasePanelEvent> for EditorHostPanel {}

/// 关闭一份文档的面板（**宿主调用**，比如 `Ctrl+W`）
///
/// 为什么是宿主的活：[`Panel::on_removed`] 会回读面板（可见性 / 可关闭性），
/// 而动作处理器本身就在面板的 `update` 里——从那里发起关闭是重入，GPUI 会直接 panic。
/// 宿主不在面板的 `update` 中，所以只有它能安全地让容器移除面板。
///
/// 返回是否真的发起关闭：脏文档会被拦下，并在面板状态栏说明原因（不静默）。
/// 文档本身由 `EditorHostPanel::on_removed` 从 `EditorService` 里关闭——
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
    area.update(cx, |area, cx| area.remove_panel(panel, window, cx));
    true
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

    /// 可关闭；关闭语义是**草稿兜底**（Dock 没有"关闭前否决"钩子，架构 §12 #18）：
    /// 关闭即落草稿，需要弹窗确认时再上自定义标签条。
    fn closable(&self, _cx: &App) -> bool {
        true
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
        self.closed = true;
        self.group = None;
        self.shared
            .update(|service| service.close(&self.document));
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
                .size(rems(ui::DIRTY_DOT_SIZE))
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
        };

        div()
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
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .px(rems(ui::EDITOR_BODY_PADDING_X))
                    .child(
                        Editor::new(&self.editor)
                            .appearance(false)
                            .bordered(false)
                            .readonly(read_only)
                            .size_full(),
                    ),
            )
            .child(status_bar::render(&status, cx))
    }
}

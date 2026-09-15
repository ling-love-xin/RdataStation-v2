//! 编辑器宿主面板（一个面板 = 一个标签 = 一份文档）
//!
//! 标签条由 Dock 提供（架构 D13 / P0.1 结论）：面板只负责标题（文档名）、`title_suffix`（脏点）
//! 与正文（编辑内核）。**不自绘标签条**——V1 自绘 35px 标签条 + 溢出菜单花了约 300 行，换来的是
//! 与 Dock 更差的一致性（拖拽排序 / 键盘导航 / 溢出都要自己再实现一遍）。
//!
//! 多文档 = 同一个 tab 组里的多个面板：`DockLayout::tabs()` 里逐个 `panel_view` 即可。

use gpui_kit::base::StyledExt as _;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::dock::{BasePanel, Panel as ComponentPanel, PanelEvent as BasePanelEvent};
use gpui_kit::component::input::{Editor, EditorState, InputEvent};
use gpui_kit::*;

use crate::model::{DocumentId, EditorMode};
use crate::service::Document;
use crate::shared::EditorShared;
use crate::ui;
use crate::view::highlight;

/// 一份文档的编辑面板
pub struct EditorHostPanel {
    shared: EditorShared,
    document: DocumentId,
    focus_handle: FocusHandle,
    /// 编辑内核状态（构造需要 `window`，故在 `new` 里一次建好）
    editor: Entity<EditorState>,
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
                    this.shared
                        .update(|service| service.set_content(&this.document, text));
                    // 重绘以刷新标签脏点与状态栏
                    cx.notify();
                }
            },
        );

        Self {
            shared,
            document,
            focus_handle,
            editor,
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

    /// 把文档内容推回内核（另存为 / 外部修改后重新加载时调用）
    pub fn reload_from_document(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(text) = self.with_document(|doc| doc.content().to_string()) else {
            return;
        };
        self.editor.update(cx, |state, cx| {
            if state.value().to_string() != text {
                state.set_value(text, window, cx);
            }
        });
        cx.notify();
    }
}

impl EventEmitter<BasePanelEvent> for EditorHostPanel {}

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
        let theme = cx.theme();
        let bg = theme.colors.background;
        let read_only = self.editor_read_only();

        div().v_flex().size_full().min_h_0().bg(bg).child(
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
    }
}

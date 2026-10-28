//! 编辑器主机面板 —— trait 实现（Focusable/BasePanel/ComponentPanel/Drop）
//!
//! 自 host/mod.rs 拆分（2026-10-29 Step 5）：`impl EditorHostPanel` 尾部与 trait 实现按职责迁出，mod.rs 收敛为核心状态与调度。
//!
use super::*;

impl EventEmitter<BasePanelEvent> for EditorHostPanel {}

impl Drop for EditorHostPanel {
    /// 面板关闭（文档随之关闭）：给语言服务发 `didClose`（尽力），别把文档留在服务端缓存里。
    ///
    /// Drop 期间不做任何面板 / 内核操作——只发一条不期待响应的通知；client 已停机时
    /// `notify` 返回错误，静默忽略。
    fn drop(&mut self) {
        if let Some(session) = self.lsp.take() {
            let _ = session.client.did_close(&session.uri);
        }
    }
}

/// 【T2c-5】第一个组的组头（组移动↑↓ 的左边缘判定）
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
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.group = Some(group);
        // 【遗留 #10】分析模式：进 Dock 时把单元编辑器建齐（事件路径；构造期不能建）
        self.sync_cell_editors(window, cx);
        // 【书签】进 Dock 读一次 sidecar（有路径才读；事件路径，render 不碰 I/O）
        self.load_bookmarks(cx);
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
        self.shared.update(|service| service.close(&self.document));
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
    ///
    /// 2026-10-19：自绘 ✕ 改为**悬停显示**（`title_hovered` 随标题区 hover 翻转），
    /// 平时只留脏点；鼠标移上标题才出现关闭按钮，避免常驻 ✕ 遮挡标题（#16）。
    fn title_suffix(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement> {
        self.is_dirty().then(|| {
            // 脏态可见（脏点 + 自绘 ✕）：
            // Dock 内置 ✕ 在脏时被 `closable` 藏掉（防 `group.close` 直接丢修改），
            // 这里补一个走「保存/不保存/取消」确认的 ✕——与 Ctrl+W 同一入口
            // （`request_close_document`），保证“编辑区也有关闭入口”且不会误丢。
            let panel = cx.entity();
            let area = self.area.clone();
            let hovered = self.title_hovered.get();
            div()
                .id(format!("sp-dirty-hover-{}", panel.entity_id()))
                .h_flex()
                .items_center()
                .gap_1()
                .on_hover({
                    // hover 闭包用 clone：不消耗 `panel`，留给下方 on_click 用。
                    let panel_hover = panel.clone();
                    move |&hovering, _, app| {
                        panel_hover.update(app, |this, cx| {
                            if this.title_hovered.get() != hovering {
                                this.title_hovered.set(hovering);
                                cx.notify();
                            }
                        });
                    }
                })
                .child(
                    div()
                        .size(rems(ui::STATUS_DOT_SIZE))
                        .rounded_full()
                        .bg(cx.theme().colors.primary),
                )
                .child(
                    Button::new(format!("sp-dirty-close-{}", panel.entity_id()))
                        .icon(IconName::Close)
                        .xsmall()
                        .ghost()
                        .tab_stop(false)
                        .opacity(if hovered { 1.0 } else { 0.0 })
                        .on_click(move |_, window, cx| {
                            cx.stop_propagation();
                            if let Some(area) = area.as_ref().and_then(|weak| weak.upgrade()) {
                                request_close_document(&area, panel.clone(), window, cx);
                            }
                        }),
                )
        })
    }
}

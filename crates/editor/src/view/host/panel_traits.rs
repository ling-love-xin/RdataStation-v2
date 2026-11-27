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

    /// 可关闭：**恒为真**（`closable` 只决定 ✕ 画不画）。
    ///
    /// 2026-11-24：改用皮肤的 `request_close` 钩子（`vendor/gpui-component` 的
    /// `dock::CloseRequest`）——脏文档的 ✕ 照常悬停显示，点击先弹「保存 / 不保存 /
    /// 取消」，只有选到"不保存/已保存"才真正移除面板。老做法 `!is_dirty()` 会把脏
    /// 标签的 ✕ 整颗藏掉：非激活的脏标签既看不到脏点也看不到关闭入口（用户实机反馈）。
    fn closable(&self, _cx: &App) -> bool {
        true
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
        // 临时结果是**实例资源**（`rds_tmp`），跨文档共享没问题；但"哪份工作建的"要追得到，
        // 否则关掉一份工作它的表永远留着（TTL 也要等半小时）。这里按 owner = 文档 id 收一批，
        // 再把这份工作的分析会话关掉（会话占着的别名 / ATTACH 随连接一起走）。
        if let Some(instance) = engine::duckdb::instance::current() {
            let owner = self.document.as_str();
            if let Err(error) = instance.drop_temps_of(owner) {
                tracing::warn!("关文档 {owner} 收临时结果失败：{error}");
            }
            if let Some(session) = instance.session_for_owner(owner) {
                instance.close_session(session.id());
            }
        }
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

    /// 【关闭前询问】标签 ✕ 点击先到这里：脏 → 交给「保存 / 不保存 / 取消」三态
    /// 确认（与 `Ctrl+W` 同一入口 [`request_close_document`]），并**否决**这次移除；
    /// 干净 → 放行，皮肤照常把面板摘掉。
    fn request_close(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui_kit::component::dock::CloseRequest {
        if !self.is_dirty() {
            return gpui_kit::component::dock::CloseRequest::Proceed;
        }
        if let Some(area) = self.area.as_ref().and_then(|weak| weak.upgrade()) {
            request_close_document(&area, cx.entity(), window, cx);
        }
        gpui_kit::component::dock::CloseRequest::Cancel
    }

    /// 面板 zoom（组铺满 DockArea）的入口位置：**标题栏内联按钮 + 三点菜单都可用**。
    ///
    /// 0.10 的 `PanelControl` 只有三档：`Menu`（默认，唯一入口埋在标题栏三点菜单里，
    /// 用户找不到）、`Toolbar`（只画内联按钮，但三点菜单会留下一条**灰掉的禁用项**，
    /// 见 `vendor/gpui-component/src/dock/tab_panel.rs` 的 `menu_with_disabled(!menu_zoom)`）、
    /// `Both`（两处都在且都可用）。这里选 `Both`：标题栏直接能点，菜单不出现"坏掉的项"。
    ///
    /// zoom 是瞬态（`DockState` / `PanelInfo` 不含 zoomed，重启不恢复），只影响该组铺满
    /// area，不涉及文档状态，因此不需要持久化配合。
    fn zoom_control(&self, _cx: &App) -> Option<PanelControl> {
        Some(PanelControl::Both)
    }

    /// 内容区顶部内距：**不要**（面板自己画了 36 高的工具栏）。
    ///
    /// Dock 的 `TabGroupSkin::content_frame` 在多面板组里给内容区加 `pt_2`（8px），
    /// 本意是给「裸内容」留呼吸；本面板第一行就是自绘工具栏，再垫 8px 会露一条
    /// 与工具栏异色的空带（用户实机反馈 2026-11-25：编辑器与右侧面板头离标题条有间隔）。
    /// 同组里的左右面板一律 `false`（`workbench/src/panels/left_tabs/*`、`right_tabs.rs`），
    /// 这里不跟着写就会「切一个 tab 内容区上下跳 8px」。
    fn inner_padding(&self, _cx: &App) -> bool {
        false
    }

    /// 【C3 状态槽】标签左端 = **固定 12px 状态槽 + 类型图标**，脏点在槽里。
    ///
    /// 顺序按 Zed：`start_slot`（状态位）在最左，随后才是文件类型图标，最后是标题。
    /// 状态槽在左的好处是它在**所有标签的同一列**上（状态位不随标题文字长短漂移），
    /// 扫一眼标签条就能看出哪些文档有未保存改动（用户实机反馈 2026-11-24：
    /// 首版把图标放前、脏点放后，与 Zed 相反）。
    ///
    /// 图标为什么在这里：中央编辑区恒为标签条形态，标签文字只走 `tab_name()`
    /// （纯字符串），标题行里 `title()` 的那套富文本用不上——所以文件 tab 此前
    /// 光秃秃只有文件名（用户实机反馈 2026-11-24「文件类型小图标不见了」）。
    /// 左侧 dock 的 tab（`tab_name() == None`）走 `title()`，图标本来就在。
    ///
    /// 槽位固定占位，变脏只换内容不改宽度，所以标签不会因为状态变化而抖动
    /// （对齐 Zed 的 `START_TAB_SLOT_SIZE = 12px`）。
    fn tab_prefix(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        let dirty = self.is_dirty();
        let icon = ui::file_type_icon(&self.title_text());
        Some(
            div()
                .h_flex()
                .items_center()
                .gap_1()
                .flex_shrink_0()
                .child(
                    div()
                        .w(rems(ui::TAB_STATUS_SLOT))
                        .flex_shrink_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .when(dirty, |this| {
                            this.child(
                                div()
                                    .size(rems(ui::STATUS_DOT_SIZE))
                                    .rounded_full()
                                    .bg(cx.theme().colors.primary),
                            )
                        }),
                )
                .child(
                    Icon::empty()
                        .path(icon)
                        .size(cx.theme().font_size * ui::TAB_TYPE_ICON),
                )
                .into_any_element(),
        )
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
